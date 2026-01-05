use chrono::{DateTime, Utc};
use db::models::{
    merge::{Merge, PullRequestInfo},
    sprint::Sprint,
    task::Task,
};
use std::collections::{BTreeMap, BTreeSet};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReleaseNotesTaskItem {
    pub task: Task,
    pub pull_requests: Vec<PullRequestInfo>,
    pub commits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReleaseNotesStats {
    pub tasks_done: usize,
    pub pull_requests: usize,
    pub commits: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReleaseNotesResponse {
    pub sprint: Sprint,
    pub markdown: String,
    pub stats: ReleaseNotesStats,
    pub tasks: Vec<ReleaseNotesTaskItem>,
}

pub fn build_task_item(task: Task, merges: Vec<Merge>) -> ReleaseNotesTaskItem {
    let mut prs_by_url: BTreeMap<String, PullRequestInfo> = BTreeMap::new();
    let mut commits_set: BTreeSet<String> = BTreeSet::new();

    for merge in merges {
        if let Some(commit) = merge.merge_commit().map(|s| s.trim().to_string()) {
            if !commit.is_empty() {
                commits_set.insert(commit);
            }
        }

        if let Merge::Pr(pr) = merge {
            prs_by_url
                .entry(pr.pr_info.url.clone())
                .or_insert(pr.pr_info);
        }
    }

    let pull_requests: Vec<PullRequestInfo> = prs_by_url.into_values().collect();
    let commits: Vec<String> = commits_set.into_iter().collect();

    ReleaseNotesTaskItem {
        task,
        pull_requests,
        commits,
    }
}

pub fn render_release_notes_markdown(sprint: &Sprint, items: &[ReleaseNotesTaskItem]) -> String {
    let (pr_count, commit_count) = counts(items);

    let mut out = String::new();
    out.push_str(&format!("# Release Notes — {}\n\n", sprint.name));
    out.push_str(&format!(
        "_{} → {}_\n\n",
        format_date(sprint.start_at),
        format_date(sprint.end_at)
    ));

    out.push_str("## Summary\n");
    out.push_str(&format!("- Completed tasks: {}\n", items.len()));
    out.push_str(&format!("- Pull requests: {}\n", pr_count));
    out.push_str(&format!("- Commits: {}\n\n", commit_count));

    out.push_str("## Highlights\n");
    out.push_str("- \n\n");

    out.push_str("## Completed\n");
    if items.is_empty() {
        out.push_str("- _No completed tasks in this sprint._\n\n");
    } else {
        for item in items {
            out.push_str(&format!("- {}\n", item.task.title));
        }
        out.push('\n');
    }

    out.push_str("## Details\n");
    if items.is_empty() {
        out.push_str("_No task details._\n\n");
    } else {
        for item in items {
            out.push_str(&format!("### {}\n", item.task.title));

            if let Some(desc) = item
                .task
                .description
                .as_ref()
                .map(|d| d.trim())
                .filter(|d| !d.is_empty())
            {
                out.push_str(&format!("{}\n\n", desc));
            }

            if !item.pull_requests.is_empty() {
                out.push_str("**Pull requests**\n");
                for pr in &item.pull_requests {
                    out.push_str(&format!("- {} (#{})\n", pr.url, pr.number));
                }
                out.push('\n');
            }

            if !item.commits.is_empty() {
                out.push_str("**Commits**\n");
                for sha in &item.commits {
                    out.push_str(&format!("- `{}`\n", short_sha(sha)));
                }
                out.push('\n');
            }
        }
    }

    out.push_str("## Metadata\n");
    out.push_str(&format!("- Generated: {}\n", Utc::now().to_rfc3339()));

    out
}

pub fn build_release_notes_response(
    sprint: Sprint,
    tasks: Vec<ReleaseNotesTaskItem>,
) -> ReleaseNotesResponse {
    let (pull_requests, commits) = counts(&tasks);

    let stats = ReleaseNotesStats {
        tasks_done: tasks.len(),
        pull_requests,
        commits,
    };

    let markdown = render_release_notes_markdown(&sprint, &tasks);

    ReleaseNotesResponse {
        sprint,
        markdown,
        stats,
        tasks,
    }
}

fn counts(items: &[ReleaseNotesTaskItem]) -> (usize, usize) {
    let mut prs: Vec<String> = items
        .iter()
        .flat_map(|i| i.pull_requests.iter().map(|p| p.url.clone()))
        .collect();
    prs.sort();
    prs.dedup();

    let mut commits: Vec<String> = items
        .iter()
        .flat_map(|i| i.commits.iter().cloned())
        .collect();
    commits.sort();
    commits.dedup();

    (prs.len(), commits.len())
}

fn short_sha(sha: &str) -> String {
    let trimmed = sha.trim();
    if trimmed.len() <= 7 {
        trimmed.to_string()
    } else {
        trimmed[..7].to_string()
    }
}

fn format_date(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use db::models::task::TaskStatus;
    use uuid::Uuid;

    fn sprint() -> Sprint {
        Sprint {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            name: "Sprint 42".to_string(),
            start_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            end_at: Utc.with_ymd_and_hms(2026, 1, 15, 0, 0, 0).unwrap(),
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        }
    }

    fn task(title: &str) -> Task {
        Task {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            title: title.to_string(),
            description: Some("Some details".to_string()),
            status: TaskStatus::Done,
            parent_workspace_id: None,
            shared_task_id: None,
            created_at: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 3, 0, 0, 0).unwrap(),
            done_at: Some(Utc.with_ymd_and_hms(2026, 1, 3, 0, 0, 0).unwrap()),
        }
    }

    fn pr(number: i64, url: &str) -> PullRequestInfo {
        PullRequestInfo {
            number,
            url: url.to_string(),
            status: db::models::merge::MergeStatus::Merged,
            merged_at: None,
            merge_commit_sha: Some("0123456789abcdef".to_string()),
        }
    }

    #[test]
    fn build_task_item_dedups_prs_and_commits() {
        let task = task("A");
        let merges = vec![
            Merge::Pr(db::models::merge::PrMerge {
                id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
                repo_id: Uuid::new_v4(),
                created_at: Utc.with_ymd_and_hms(2026, 1, 3, 0, 0, 0).unwrap(),
                target_branch_name: "main".to_string(),
                pr_info: pr(1, "https://example.com/pull/1"),
            }),
            Merge::Pr(db::models::merge::PrMerge {
                id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
                repo_id: Uuid::new_v4(),
                created_at: Utc.with_ymd_and_hms(2026, 1, 4, 0, 0, 0).unwrap(),
                target_branch_name: "main".to_string(),
                pr_info: pr(1, "https://example.com/pull/1"),
            }),
            Merge::Direct(db::models::merge::DirectMerge {
                id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
                repo_id: Uuid::new_v4(),
                created_at: Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap(),
                target_branch_name: "main".to_string(),
                merge_commit: "aaaaaaaaaaaa".to_string(),
            }),
            Merge::Direct(db::models::merge::DirectMerge {
                id: Uuid::new_v4(),
                workspace_id: Uuid::new_v4(),
                repo_id: Uuid::new_v4(),
                created_at: Utc.with_ymd_and_hms(2026, 1, 6, 0, 0, 0).unwrap(),
                target_branch_name: "main".to_string(),
                merge_commit: "aaaaaaaaaaaa".to_string(),
            }),
        ];

        let item = build_task_item(task, merges);
        assert_eq!(item.pull_requests.len(), 1, "{:?}", item.pull_requests);
        assert_eq!(item.commits.len(), 2, "{:?}", item.commits);
    }

    #[test]
    fn markdown_contains_sections_and_counts() {
        let sprint = sprint();
        let items = vec![
            ReleaseNotesTaskItem {
                task: task("Add thing"),
                pull_requests: vec![pr(1, "https://example.com/pull/1")],
                commits: vec!["aaaaaaaaaaaa".to_string()],
            },
            ReleaseNotesTaskItem {
                task: task("Fix bug"),
                pull_requests: vec![
                    pr(1, "https://example.com/pull/1"), // duplicate across tasks
                    pr(2, "https://example.com/pull/2"),
                ],
                commits: vec![
                    "aaaaaaaaaaaa".to_string(), // duplicate across tasks
                    "bbbbbbb".to_string(),
                ],
            },
        ];

        let md = render_release_notes_markdown(&sprint, &items);
        assert!(md.contains("# Release Notes — Sprint 42"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("- Completed tasks: 2"));
        assert!(md.contains("- Pull requests: 2"));
        assert!(md.contains("- Commits: 2"));
        assert!(md.contains("## Completed"));
        assert!(md.contains("## Details"));
        assert!(md.contains("### Add thing"));
        assert!(md.contains("### Fix bug"));
    }

    #[test]
    fn short_sha_truncates() {
        assert_eq!(short_sha("0123456789abcdef"), "0123456");
        assert_eq!(short_sha("abcdefg"), "abcdefg");
    }
}
