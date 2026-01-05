use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use ts_rs::TS;
use uuid::Uuid;

use crate::{
    env::ExecutionEnv,
    executors::{
        ExecutorError,
        StandardCodingAgentExecutor,
        codex::{AskForApproval, Codex, SandboxMode},
    },
    logs::{
        NormalizedEntryError, NormalizedEntryType,
        utils::patch::extract_normalized_entry_from_patch,
    },
};
use workspace_utils::msg_store::MsgStore;

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema, PartialEq, Eq)]
#[ts(export)]
pub struct BacklogGroomingDraft {
    /// A short list of testable outcomes.
    pub acceptance_criteria: Vec<String>,
    /// 3-5 atomic pieces of work.
    pub subtasks: Vec<String>,
    /// A story-point estimate (Fibonacci-ish: 1,2,3,5,8,13).
    pub story_points: u8,
}

impl BacklogGroomingDraft {
    pub fn sanitize(mut self) -> Self {
        self.acceptance_criteria = sanitize_items(self.acceptance_criteria, 1, 10);
        self.subtasks = sanitize_items(self.subtasks, 0, 10);
        if self.subtasks.len() > 5 {
            self.subtasks.truncate(5);
        }
        if self.subtasks.len() < 3 {
            // Keep it non-empty; caller should validate strictness.
            self.subtasks = pad_subtasks(self.subtasks);
        }
        self.story_points = normalize_story_points(self.story_points);
        self
    }

    pub fn validate_strict(&self) -> Result<(), &'static str> {
        if self.acceptance_criteria.is_empty() {
            return Err("acceptance_criteria must be non-empty");
        }
        if self.subtasks.len() < 3 || self.subtasks.len() > 5 {
            return Err("subtasks must be 3-5 items");
        }
        if !is_allowed_story_points(self.story_points) {
            return Err("story_points must be one of 1,2,3,5,8,13");
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BacklogGroomerError {
    #[error("codex auth required")]
    AuthRequired,
    #[error("executor error: {0}")]
    Executor(#[from] ExecutorError),
    #[error("timed out waiting for model output")]
    Timeout,
    #[error("failed to extract assistant message")]
    MissingAssistantMessage,
    #[error("failed to extract JSON from assistant message")]
    MissingJson,
    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid draft: {0}")]
    InvalidDraft(&'static str),
    #[error("unsupported coding agent for backlog grooming")]
    UnsupportedAgent,
}

#[derive(Debug, Clone)]
pub struct BacklogGroomerLimits {
    pub timeout: Duration,
    pub max_story_chars: usize,
}

impl Default for BacklogGroomerLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(45),
            max_story_chars: 4000,
        }
    }
}

pub struct BacklogGroomer {
    pub limits: BacklogGroomerLimits,
}

impl Default for BacklogGroomer {
    fn default() -> Self {
        Self {
            limits: BacklogGroomerLimits::default(),
        }
    }
}

impl BacklogGroomer {
    pub fn build_prompt(story: &str) -> String {
        // Keep this prompt tool-free: no files, no commands.
        [
            "You are a backlog grooming assistant.",
            "Given a single user story, produce STRICT JSON with keys:",
            r#"- "acceptance_criteria": string[] (3-7 items, testable, concise)"#,
            r#"- "subtasks": string[] (3-5 items, atomic, actionable)"#,
            r#"- "story_points": number (one of 1,2,3,5,8,13)"#,
            "Rules:",
            "- Output ONLY valid JSON (no markdown, no code fences, no commentary).",
            "- Do not use tools, commands, or file operations.",
            "- Keep each string <= 280 chars.",
            "",
            "STORY:",
            story.trim(),
        ]
        .join("\n")
    }

    pub async fn generate_with_codex(
        &self,
        story: &str,
        mut codex: Codex,
        env: &ExecutionEnv,
    ) -> Result<BacklogGroomingDraft, BacklogGroomerError> {
        let story = truncate_chars(story.trim().to_string(), self.limits.max_story_chars);
        let prompt = Self::build_prompt(&story);

        // Force a safe, tool-free Codex session.
        codex.include_apply_patch_tool = Some(false);
        codex.sandbox = Some(SandboxMode::ReadOnly);
        codex.ask_for_approval = Some(AskForApproval::Never);

        let tmp_dir = std::env::temp_dir().join(format!("vk-backlog-groomer-{}", Uuid::new_v4()));
        let _tmp_guard = TmpDirCleanup::new(tmp_dir.clone());
        tokio::fs::create_dir_all(&tmp_dir)
            .await
            .map_err(|e| ExecutorError::Io(e))?;

        let msg_store = Arc::new(MsgStore::new());
        crate::executors::codex::normalize_logs::normalize_logs(msg_store.clone(), &tmp_dir);

        let mut spawned = codex.spawn(&tmp_dir, &prompt, env).await?;

        let mut stdout = spawned
            .child
            .inner()
            .stdout
            .take()
            .ok_or_else(|| ExecutorError::Io(std::io::Error::other("missing stdout")))?;
        let mut stderr = spawned
            .child
            .inner()
            .stderr
            .take()
            .ok_or_else(|| ExecutorError::Io(std::io::Error::other("missing stderr")))?;

        let stdout_store = msg_store.clone();
        let stderr_store = msg_store.clone();
        let stdout_task = tokio::spawn(async move {
            let mut buf = [0u8; 8192];
            while let Ok(n) = stdout.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                stdout_store.push_stdout(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = [0u8; 8192];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                stderr_store.push_stderr(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
        });

        let exit = async {
            if let Some(exit_signal) = spawned.exit_signal.take() {
                let res = tokio::time::timeout(self.limits.timeout, exit_signal)
                    .await
                    .map_err(|_| BacklogGroomerError::Timeout)?;
                // If the oneshot was dropped, treat it as failure but still attempt extraction.
                let _ = res;
            } else {
                tokio::time::sleep(self.limits.timeout).await;
            }
            Ok::<(), BacklogGroomerError>(())
        };

        if let Err(err) = exit.await {
            let _ = spawned.child.kill().await;
            let _ = spawned.child.wait().await;
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            msg_store.push_finished();
            return Err(err);
        }

        // Stop the app-server process once task_complete has fired.
        let _ = spawned.child.kill().await;
        let _ = spawned.child.wait().await;
        let _ = stdout_task.await;
        let _ = stderr_task.await;
        msg_store.push_finished();

        // Give the normalizer a moment to emit the final patch.
        tokio::time::sleep(Duration::from_millis(150)).await;

        if has_setup_required_error(&msg_store) {
            return Err(BacklogGroomerError::AuthRequired);
        }

        let assistant = extract_last_assistant_message(&msg_store)
            .ok_or(BacklogGroomerError::MissingAssistantMessage)?;
        let json_str = extract_first_json_object(&assistant).ok_or(BacklogGroomerError::MissingJson)?;

        let draft: BacklogGroomingDraft = serde_json::from_str(&json_str)?;
        let draft = draft.sanitize();
        draft
            .validate_strict()
            .map_err(BacklogGroomerError::InvalidDraft)?;
        Ok(draft)
    }
}

fn extract_last_assistant_message(msg_store: &Arc<MsgStore>) -> Option<String> {
    let mut last: Option<(usize, String)> = None;
    for msg in msg_store.get_history() {
        let workspace_utils::log_msg::LogMsg::JsonPatch(patch) = msg else {
            continue;
        };
        let Some((idx, entry)) = extract_normalized_entry_from_patch(&patch) else {
            continue;
        };
        if matches!(entry.entry_type, NormalizedEntryType::AssistantMessage) {
            match &last {
                Some((prev_idx, _)) if *prev_idx >= idx => {}
                _ => last = Some((idx, entry.content)),
            }
        }
    }
    last.map(|(_, s)| s)
}

fn has_setup_required_error(msg_store: &Arc<MsgStore>) -> bool {
    for msg in msg_store.get_history() {
        let workspace_utils::log_msg::LogMsg::JsonPatch(patch) = msg else {
            continue;
        };
        let Some((_idx, entry)) = extract_normalized_entry_from_patch(&patch) else {
            continue;
        };
        if matches!(
            entry.entry_type,
            NormalizedEntryType::ErrorMessage {
                error_type: NormalizedEntryError::SetupRequired
            }
        ) {
            return true;
        }
    }
    false
}

fn extract_first_json_object(text: &str) -> Option<String> {
    // Prefer fenced ```json blocks when present.
    if let Some(start) = text.find("```json") {
        let after = &text[start + "```json".len()..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim().to_string());
        }
    }
    // Fallback: first {...} span.
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(text[start..=end].trim().to_string())
}

#[allow(dead_code)]
fn ensure_workdir_exists(path: &Path) -> Result<(), ExecutorError> {
    std::fs::create_dir_all(path).map_err(ExecutorError::Io)
}

struct TmpDirCleanup {
    path: PathBuf,
}

impl TmpDirCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TmpDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn sanitize_items(items: Vec<String>, min: usize, max: usize) -> Vec<String> {
    let mut out: Vec<String> = items
        .into_iter()
        .map(|s| s.trim().trim_start_matches('-').trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, 280))
        .collect();
    if out.len() > max {
        out.truncate(max);
    }
    if out.len() < min {
        out = out.into_iter().take(min).collect();
    }
    out
}

fn pad_subtasks(mut subtasks: Vec<String>) -> Vec<String> {
    while subtasks.len() < 3 {
        subtasks.push("TBD".to_string());
    }
    subtasks
}

fn is_allowed_story_points(value: u8) -> bool {
    matches!(value, 1 | 2 | 3 | 5 | 8 | 13)
}

fn normalize_story_points(value: u8) -> u8 {
    if is_allowed_story_points(value) {
        return value;
    }
    // Nearest bucket, conservative bias upward.
    match value {
        0 => 1,
        1..=2 => 2,
        3..=4 => 3,
        5..=7 => 5,
        8..=12 => 8,
        _ => 13,
    }
}

fn truncate_chars(mut s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    s = s.chars().take(max).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_enforces_basic_constraints() {
        let draft = BacklogGroomingDraft {
            acceptance_criteria: vec!["  -  ".to_string(), " ok ".to_string()],
            subtasks: vec!["a".to_string()],
            story_points: 99,
        }
        .sanitize();

        assert_eq!(draft.acceptance_criteria, vec!["ok".to_string()]);
        assert_eq!(draft.subtasks.len(), 3);
        assert_eq!(draft.story_points, 13);
    }

    #[test]
    fn validate_strict_checks_counts_and_points() {
        let ok = BacklogGroomingDraft {
            acceptance_criteria: vec!["a".to_string()],
            subtasks: vec!["1".to_string(), "2".to_string(), "3".to_string()],
            story_points: 5,
        };
        assert!(ok.validate_strict().is_ok());

        let bad = BacklogGroomingDraft {
            acceptance_criteria: vec![],
            subtasks: vec!["1".to_string(), "2".to_string(), "3".to_string()],
            story_points: 5,
        };
        assert!(bad.validate_strict().is_err());
    }

    #[test]
    fn extract_json_prefers_fenced_block() {
        let text = "hi\n```json\n{\"a\":1}\n```\nbye";
        assert_eq!(extract_first_json_object(text).unwrap(), "{\"a\":1}");
    }
}
