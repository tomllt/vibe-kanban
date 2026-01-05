use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json as ResponseJson,
    routing::post,
};
use deployment::Deployment;
use db::models::{
    merge::{Merge, MergeStatus},
    task::{Task, TaskStatus},
    workspace::Workspace,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utils::response::ApiResponse;

use crate::DeploymentImpl;

const ENV_GITHUB_SECRET_PRIMARY: &str = "GITHUB_WEBHOOK_SECRET";
const ENV_GITHUB_SECRET_FALLBACK: &str = "GITHUB_APP_WEBHOOK_SECRET";

const ENV_GITLAB_TOKEN_PRIMARY: &str = "GITLAB_WEBHOOK_TOKEN";
const ENV_GITLAB_TOKEN_FALLBACK: &str = "GITLAB_WEBHOOK_SECRET";

const MAX_STORED_PAYLOAD_BYTES: usize = 200_000;

#[derive(Debug, Serialize)]
pub struct WebhookAck {
    provider: &'static str,
    event: String,
    delivery_id: String,
    status: &'static str,
    deduped: bool,
    updated_merges: usize,
    updated_tasks: usize,
}

pub fn router() -> Router<DeploymentImpl> {
    let webhooks = Router::new()
        .route("/github", post(github_webhook))
        .route("/gitlab", post(gitlab_webhook));

    Router::new().nest("/webhooks", webhooks)
}

async fn github_webhook(
    State(deployment): State<DeploymentImpl>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, ResponseJson<ApiResponse<WebhookAck>>) {
    let Some(event) = header_str(&headers, "X-GitHub-Event") else {
        return bad_request("missing X-GitHub-Event");
    };
    let Some(delivery_id) = header_str(&headers, "X-GitHub-Delivery") else {
        return bad_request("missing X-GitHub-Delivery");
    };
    let Some(signature_header) = header_str(&headers, "X-Hub-Signature-256") else {
        return unauthorized("missing X-Hub-Signature-256");
    };

    let Some(secret) = read_env_secret(ENV_GITHUB_SECRET_PRIMARY, ENV_GITHUB_SECRET_FALLBACK) else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            ResponseJson(ApiResponse::error(
                "GitHub webhooks not configured (missing webhook secret)",
            )),
        );
    };

    let signature_valid =
        remote::github_app::verify_webhook_signature(secret.as_bytes(), &signature_header, &body);
    if !signature_valid {
        return unauthorized("invalid signature");
    }

    let pool = &deployment.db().pool;

    let inserted = match upsert_delivery(
        pool,
        "github",
        &delivery_id,
        &event,
        signature_valid,
        &body,
    )
    .await
    {
        Ok(inserted) => inserted,
        Err(err) => {
            tracing::error!(?err, "failed to store webhook delivery");
            return internal_error("failed to store delivery");
        }
    };

    if !inserted {
        return ok(WebhookAck {
            provider: "github",
            event,
            delivery_id,
            status: "duplicate",
            deduped: true,
            updated_merges: 0,
            updated_tasks: 0,
        });
    }

    let result = process_github_event(&deployment, &event, &body).await;
    match result {
        Ok(ProcessResult {
            status,
            updated_merges,
            updated_tasks,
        }) => {
            if let Err(err) =
                mark_delivery(pool, "github", &delivery_id, status, None).await
            {
                tracing::warn!(?err, "failed to mark webhook delivery status");
            }
            let ack = WebhookAck {
                provider: "github",
                event: event.clone(),
                delivery_id: delivery_id.clone(),
                status,
                deduped: false,
                updated_merges,
                updated_tasks,
            };
            tracing::info!(
                provider = "github",
                event = %event,
                delivery_id = %delivery_id,
                status = %status,
                updated_merges = updated_merges,
                updated_tasks = updated_tasks,
                "webhook processed"
            );
            ok(ack)
        }
        Err(err) => {
            tracing::error!(?err, event = %event, delivery_id = %delivery_id, "webhook processing failed");
            if let Err(db_err) =
                mark_delivery(pool, "github", &delivery_id, "failed", Some(&err)).await
            {
                tracing::warn!(?db_err, "failed to mark webhook delivery failed");
            }
            internal_error("webhook processing failed")
        }
    }
}

async fn gitlab_webhook(
    State(deployment): State<DeploymentImpl>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, ResponseJson<ApiResponse<WebhookAck>>) {
    let Some(event) = header_str(&headers, "X-Gitlab-Event") else {
        return bad_request("missing X-Gitlab-Event");
    };
    let Some(delivery_id) = header_str(&headers, "X-Gitlab-Event-UUID")
        .or_else(|| header_str(&headers, "X-Request-Id"))
    else {
        return bad_request("missing X-Gitlab-Event-UUID (or X-Request-Id)");
    };

    let Some(token_header) = header_str(&headers, "X-Gitlab-Token") else {
        return unauthorized("missing X-Gitlab-Token");
    };

    let Some(expected) = read_env_secret(ENV_GITLAB_TOKEN_PRIMARY, ENV_GITLAB_TOKEN_FALLBACK) else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            ResponseJson(ApiResponse::error(
                "GitLab webhooks not configured (missing webhook token)",
            )),
        );
    };

    if !constant_time_eq(token_header.as_bytes(), expected.as_bytes()) {
        return unauthorized("invalid token");
    }

    let pool = &deployment.db().pool;

    let inserted =
        match upsert_delivery(pool, "gitlab", &delivery_id, &event, true, &body).await {
            Ok(inserted) => inserted,
            Err(err) => {
                tracing::error!(?err, "failed to store webhook delivery");
                return internal_error("failed to store delivery");
            }
        };

    if !inserted {
        return ok(WebhookAck {
            provider: "gitlab",
            event,
            delivery_id,
            status: "duplicate",
            deduped: true,
            updated_merges: 0,
            updated_tasks: 0,
        });
    }

    let result = process_gitlab_event(&deployment, &event, &body).await;
    match result {
        Ok(ProcessResult {
            status,
            updated_merges,
            updated_tasks,
        }) => {
            if let Err(err) = mark_delivery(pool, "gitlab", &delivery_id, status, None).await {
                tracing::warn!(?err, "failed to mark webhook delivery status");
            }
            let ack = WebhookAck {
                provider: "gitlab",
                event: event.clone(),
                delivery_id: delivery_id.clone(),
                status,
                deduped: false,
                updated_merges,
                updated_tasks,
            };
            tracing::info!(
                provider = "gitlab",
                event = %event,
                delivery_id = %delivery_id,
                status = %status,
                updated_merges = updated_merges,
                updated_tasks = updated_tasks,
                "webhook processed"
            );
            ok(ack)
        }
        Err(err) => {
            tracing::error!(?err, event = %event, delivery_id = %delivery_id, "webhook processing failed");
            if let Err(db_err) =
                mark_delivery(pool, "gitlab", &delivery_id, "failed", Some(&err)).await
            {
                tracing::warn!(?db_err, "failed to mark webhook delivery failed");
            }
            internal_error("webhook processing failed")
        }
    }
}

struct ProcessResult {
    status: &'static str,
    updated_merges: usize,
    updated_tasks: usize,
}

async fn process_github_event(
    deployment: &DeploymentImpl,
    event: &str,
    body: &[u8],
) -> anyhow::Result<ProcessResult> {
    match event {
        "pull_request" => {
            let payload: GitHubPullRequestEvent = serde_json::from_slice(body)?;
            process_github_pull_request(deployment, payload).await
        }
        "push" => {
            let payload: GitHubPushEvent = serde_json::from_slice(body)?;
            upsert_repo_state_github_push(&deployment.db().pool, payload).await?;
            Ok(ProcessResult {
                status: "processed",
                updated_merges: 0,
                updated_tasks: 0,
            })
        }
        "check_run" => {
            let payload: GitHubCheckRunEvent = serde_json::from_slice(body)?;
            upsert_repo_state_github_check_run(&deployment.db().pool, payload).await?;
            Ok(ProcessResult {
                status: "processed",
                updated_merges: 0,
                updated_tasks: 0,
            })
        }
        _ => Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        }),
    }
}

async fn process_gitlab_event(
    deployment: &DeploymentImpl,
    _event: &str,
    body: &[u8],
) -> anyhow::Result<ProcessResult> {
    // GitLab event naming is inconsistent across hook types, so key off payload content.
    let base: GitLabBaseEvent = serde_json::from_slice(body)?;
    match base.object_kind.as_str() {
        "merge_request" => {
            let payload: GitLabMergeRequestEvent = serde_json::from_slice(body)?;
            process_gitlab_merge_request(deployment, payload).await
        }
        "push" => {
            let payload: GitLabPushEvent = serde_json::from_slice(body)?;
            upsert_repo_state_gitlab_push(&deployment.db().pool, payload).await?;
            Ok(ProcessResult {
                status: "processed",
                updated_merges: 0,
                updated_tasks: 0,
            })
        }
        _ => Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        }),
    }
}

async fn process_github_pull_request(
    deployment: &DeploymentImpl,
    payload: GitHubPullRequestEvent,
) -> anyhow::Result<ProcessResult> {
    let next_status = github_pr_status(&payload.pull_request);
    let pr_url = payload.pull_request.html_url;
    let merge_commit_sha = payload.pull_request.merge_commit_sha.clone();

    let pool = &deployment.db().pool;
    let (updated_merges, updated_task_ids) = update_pr_merges_and_tasks(
        pool,
        &pr_url,
        next_status.clone(),
        merge_commit_sha,
    )
    .await?;

    if updated_merges == 0 {
        return Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        });
    }

    if matches!(next_status, MergeStatus::Merged) {
        for task_id in &updated_task_ids {
            if let Ok(publisher) = deployment.share_publisher() {
                if let Err(err) = publisher.update_shared_task_by_id(*task_id).await {
                    tracing::warn!(
                        ?err,
                        task_id = %task_id,
                        "failed to propagate shared task update after webhook"
                    );
                }
            }
        }
    }

    Ok(ProcessResult {
        status: "processed",
        updated_merges,
        updated_tasks: updated_task_ids.len(),
    })
}

async fn process_gitlab_merge_request(
    deployment: &DeploymentImpl,
    payload: GitLabMergeRequestEvent,
) -> anyhow::Result<ProcessResult> {
    let Some(attrs) = payload.object_attributes else {
        return Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        });
    };

    let next_status = gitlab_mr_status(&attrs);
    let pr_url = gitlab_merge_request_url(&attrs).to_string();
    let merge_commit_sha = attrs.merge_commit_sha;

    let pool = &deployment.db().pool;
    let (updated_merges, updated_task_ids) =
        update_pr_merges_and_tasks(pool, &pr_url, next_status.clone(), merge_commit_sha).await?;

    if updated_merges == 0 {
        return Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        });
    }

    if matches!(next_status, MergeStatus::Merged) {
        for task_id in &updated_task_ids {
            if let Ok(publisher) = deployment.share_publisher() {
                if let Err(err) = publisher.update_shared_task_by_id(*task_id).await {
                    tracing::warn!(
                        ?err,
                        task_id = %task_id,
                        "failed to propagate shared task update after webhook"
                    );
                }
            }
        }
    }

    Ok(ProcessResult {
        status: "processed",
        updated_merges,
        updated_tasks: updated_task_ids.len(),
    })
}

async fn update_pr_merges_and_tasks(
    pool: &sqlx::SqlitePool,
    pr_url: &str,
    next_status: MergeStatus,
    merge_commit_sha: Option<String>,
) -> anyhow::Result<(usize, Vec<uuid::Uuid>)> {
    let merges = Merge::find_pr_by_url(pool, pr_url).await?;
    if merges.is_empty() {
        return Ok((0, vec![]));
    }

    let mut updated_merges = 0usize;
    let mut task_ids = std::collections::HashSet::<uuid::Uuid>::new();

    for pr_merge in merges {
        Merge::update_status(
            pool,
            pr_merge.id,
            next_status.clone(),
            merge_commit_sha.clone(),
        )
        .await?;
        updated_merges += 1;

        if matches!(next_status, MergeStatus::Merged) {
            if let Some(workspace) = Workspace::find_by_id(pool, pr_merge.workspace_id).await? {
                Task::update_status(pool, workspace.task_id, TaskStatus::Done).await?;
                task_ids.insert(workspace.task_id);
            }
        }
    }

    Ok((updated_merges, task_ids.into_iter().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::models::{
        project::{CreateProject, Project},
        repo::Repo,
        task::CreateTask,
        workspace::CreateWorkspace,
    };
    use sqlx::SqlitePool;
    use std::path::Path;

    async fn setup_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn updates_pr_merge_and_marks_task_done_when_merged() {
        let pool = setup_db().await;

        let project_id = uuid::Uuid::new_v4();
        let project = Project::create(
            &pool,
            &CreateProject {
                name: "p".to_string(),
                repositories: vec![],
            },
            project_id,
        )
        .await
        .unwrap();

        let task_id = uuid::Uuid::new_v4();
        let task = Task::create(
            &pool,
            &CreateTask {
                title: "t".to_string(),
                description: None,
                status: None,
                project_id: project.id,
                parent_workspace_id: None,
                shared_task_id: None,
                image_ids: None,
            },
            task_id,
        )
        .await
        .unwrap();

        let workspace_id = uuid::Uuid::new_v4();
        let workspace = Workspace::create(
            &pool,
            &CreateWorkspace {
                branch: "feature".to_string(),
                agent_working_dir: None,
            },
            workspace_id,
            task.id,
        )
        .await
        .unwrap();

        let repo = Repo::find_or_create(&pool, Path::new("/tmp/repo"), "repo")
            .await
            .unwrap();

        let pr_url = "https://github.com/o/r/pull/123";
        let pr_merge = Merge::create_pr(&pool, workspace.id, repo.id, "main", 123, pr_url)
            .await
            .unwrap();

        // Baseline: open
        let before = Merge::find_by_workspace_and_repo_id(&pool, workspace.id, repo.id)
            .await
            .unwrap();
        assert!(matches!(
            before.first().unwrap(),
            Merge::Pr(p) if p.id == pr_merge.id && matches!(p.pr_info.status, MergeStatus::Open)
        ));

        let (updated_merges, updated_tasks) = update_pr_merges_and_tasks(
            &pool,
            pr_url,
            MergeStatus::Merged,
            Some("deadbeef".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(updated_merges, 1);
        assert_eq!(updated_tasks.len(), 1);
        assert_eq!(updated_tasks[0], task.id);

        let after = Merge::find_by_workspace_and_repo_id(&pool, workspace.id, repo.id)
            .await
            .unwrap();
        match after.first().unwrap() {
            Merge::Pr(p) => {
                assert!(matches!(p.pr_info.status, MergeStatus::Merged));
                assert_eq!(p.pr_info.merge_commit_sha.as_deref(), Some("deadbeef"));
            }
            _ => panic!("expected PR merge"),
        }

        let updated_task = Task::find_by_id(&pool, task.id).await.unwrap().unwrap();
        assert!(matches!(updated_task.status, TaskStatus::Done));
    }

    #[tokio::test]
    async fn upsert_delivery_dedupes_and_counts_attempts() {
        let pool = setup_db().await;

        let inserted = upsert_delivery(
            &pool,
            "github",
            "deliv-1",
            "pull_request",
            true,
            b"{}",
        )
        .await
        .unwrap();
        assert!(inserted);

        let inserted2 = upsert_delivery(
            &pool,
            "github",
            "deliv-1",
            "pull_request",
            true,
            b"{}",
        )
        .await
        .unwrap();
        assert!(!inserted2);

        let attempts: i64 = sqlx::query_scalar(
            r#"SELECT attempts FROM webhook_deliveries WHERE provider = ? AND delivery_id = ?"#,
        )
        .bind("github")
        .bind("deliv-1")
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(attempts, 2);
    }

    #[tokio::test]
    async fn upsert_delivery_stores_hash_and_truncation_metadata() {
        let pool = setup_db().await;

        let mut payload = vec![b'a'; MAX_STORED_PAYLOAD_BYTES + 10];
        payload[0] = b'{';
        payload[1] = b'}';

        let inserted = upsert_delivery(
            &pool,
            "github",
            "deliv-big",
            "pull_request",
            true,
            &payload,
        )
        .await
        .unwrap();
        assert!(inserted);

        #[derive(sqlx::FromRow)]
        struct Row {
            payload_sha256: Option<String>,
            payload_bytes: i64,
            payload_truncated: i64,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"SELECT payload_sha256, payload_bytes, payload_truncated
               FROM webhook_deliveries
               WHERE provider = ? AND delivery_id = ?"#,
        )
        .bind("github")
        .bind("deliv-big")
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(row.payload_sha256.is_some());
        assert_eq!(row.payload_bytes, (MAX_STORED_PAYLOAD_BYTES + 10) as i64);
        assert_eq!(row.payload_truncated, 1);
    }

    #[tokio::test]
    async fn upsert_repo_state_merges_push_and_check_run() {
        let pool = setup_db().await;

        upsert_repo_state_github_push(
            &pool,
            GitHubPushEvent {
                git_ref: "refs/heads/main".to_string(),
                after: "1111111".to_string(),
                repository: GitHubRepo {
                    full_name: "o/r".to_string(),
                },
            },
        )
        .await
        .unwrap();

        upsert_repo_state_github_check_run(
            &pool,
            GitHubCheckRunEvent {
                repository: GitHubRepo {
                    full_name: "o/r".to_string(),
                },
                check_run: GitHubCheckRun {
                    head_sha: "2222222".to_string(),
                    status: Some("completed".to_string()),
                    conclusion: Some("success".to_string()),
                },
            },
        )
        .await
        .unwrap();

        #[derive(sqlx::FromRow)]
        struct Row {
            last_push_ref: Option<String>,
            last_push_sha: Option<String>,
            last_check_run_sha: Option<String>,
            last_check_run_status: Option<String>,
            last_check_run_conclusion: Option<String>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"SELECT
                    last_push_ref,
                    last_push_sha,
                    last_check_run_sha,
                    last_check_run_status,
                    last_check_run_conclusion
               FROM webhook_repo_states
               WHERE provider = ? AND repo_key = ?"#,
        )
        .bind("github")
        .bind("o/r")
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.last_push_ref.as_deref(), Some("refs/heads/main"));
        assert_eq!(row.last_push_sha.as_deref(), Some("1111111"));
        assert_eq!(row.last_check_run_sha.as_deref(), Some("2222222"));
        assert_eq!(row.last_check_run_status.as_deref(), Some("completed"));
        assert_eq!(row.last_check_run_conclusion.as_deref(), Some("success"));
    }

    #[test]
    fn gitlab_url_prefers_web_url() {
        let attrs = GitLabMergeRequestAttributes {
            url: "https://gitlab.example/api/v4/projects/1/merge_requests/2".to_string(),
            web_url: Some("https://gitlab.example/group/proj/-/merge_requests/2".to_string()),
            state: "opened".to_string(),
            merged_at: None,
            merge_commit_sha: None,
        };

        assert_eq!(
            gitlab_merge_request_url(&attrs),
            "https://gitlab.example/group/proj/-/merge_requests/2"
        );
    }
}

#[derive(Debug, Deserialize)]
struct GitHubPullRequestEvent {
    pull_request: GitHubPullRequest,
}

#[derive(Debug, Deserialize)]
struct GitHubRepo {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct GitHubPushEvent {
    #[serde(rename = "ref")]
    git_ref: String,
    after: String,
    repository: GitHubRepo,
}

#[derive(Debug, Deserialize)]
struct GitHubCheckRunEvent {
    check_run: GitHubCheckRun,
    repository: GitHubRepo,
}

#[derive(Debug, Deserialize)]
struct GitHubCheckRun {
    head_sha: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubPullRequest {
    html_url: String,
    state: String,
    merged: bool,
    merge_commit_sha: Option<String>,
}

fn github_pr_status(pr: &GitHubPullRequest) -> MergeStatus {
    if pr.merged {
        return MergeStatus::Merged;
    }
    match pr.state.as_str() {
        "open" => MergeStatus::Open,
        "closed" => MergeStatus::Closed,
        _ => MergeStatus::Unknown,
    }
}

#[derive(Debug, Deserialize)]
struct GitLabBaseEvent {
    object_kind: String,
}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequestEvent {
    #[serde(default)]
    object_attributes: Option<GitLabMergeRequestAttributes>,
}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequestAttributes {
    url: String,
    #[serde(default)]
    web_url: Option<String>,
    state: String,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    merge_commit_sha: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabPushEvent {
    #[serde(rename = "ref")]
    git_ref: String,
    #[serde(default)]
    checkout_sha: Option<String>,
    #[serde(default)]
    after: Option<String>,
    project: GitLabProject,
}

#[derive(Debug, Deserialize)]
struct GitLabProject {
    path_with_namespace: String,
}

fn gitlab_mr_status(attrs: &GitLabMergeRequestAttributes) -> MergeStatus {
    if attrs.merged_at.is_some() {
        return MergeStatus::Merged;
    }
    match attrs.state.as_str() {
        "opened" => MergeStatus::Open,
        "merged" => MergeStatus::Merged,
        "closed" => MergeStatus::Closed,
        _ => MergeStatus::Unknown,
    }
}

fn gitlab_merge_request_url(attrs: &GitLabMergeRequestAttributes) -> &str {
    attrs.web_url.as_deref().unwrap_or(&attrs.url)
}

fn read_env_secret(primary: &str, fallback: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var(fallback).ok().filter(|s| !s.trim().is_empty()))
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn upsert_delivery(
    pool: &sqlx::SqlitePool,
    provider: &str,
    delivery_id: &str,
    event: &str,
    signature_valid: bool,
    payload: &[u8],
) -> Result<bool, sqlx::Error> {
    fn bytes_to_hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            write!(&mut s, "{:02x}", b).expect("hex write should not fail");
        }
        s
    }

    let id = uuid::Uuid::new_v4();
    let payload_bytes = payload.len() as i64;
    let payload_sha256 = bytes_to_hex(Sha256::digest(payload).as_ref());

    let payload_truncated = payload.len() > MAX_STORED_PAYLOAD_BYTES;
    let payload_slice = if payload_truncated {
        &payload[..MAX_STORED_PAYLOAD_BYTES]
    } else {
        payload
    };
    let payload_json = String::from_utf8_lossy(payload_slice).to_string();

    let result = sqlx::query(
        r#"INSERT INTO webhook_deliveries (
                id,
                provider,
                delivery_id,
                event,
                signature_valid,
                payload_sha256,
                payload_bytes,
                payload_truncated,
                status,
                attempts,
                payload_json
           ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'received', 1, ?)
           ON CONFLICT(provider, delivery_id) DO NOTHING"#,
    )
    .bind(id)
    .bind(provider)
    .bind(delivery_id)
    .bind(event)
    .bind(signature_valid as i64)
    .bind(payload_sha256)
    .bind(payload_bytes)
    .bind(payload_truncated as i64)
    .bind(payload_json)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        sqlx::query(
            r#"UPDATE webhook_deliveries
               SET attempts = attempts + 1,
                   received_at = datetime('now', 'subsec')
               WHERE provider = ? AND delivery_id = ?"#,
        )
        .bind(provider)
        .bind(delivery_id)
        .execute(pool)
        .await?;
        Ok(false)
    } else {
        Ok(true)
    }
}

async fn mark_delivery(
    pool: &sqlx::SqlitePool,
    provider: &str,
    delivery_id: &str,
    status: &str,
    last_error: Option<&anyhow::Error>,
) -> Result<(), sqlx::Error> {
    let err_str = last_error.map(|e| e.to_string());
    sqlx::query(
        r#"UPDATE webhook_deliveries
           SET status = ?,
               processed_at = datetime('now', 'subsec'),
               last_error = ?
           WHERE provider = ? AND delivery_id = ?"#,
    )
    .bind(status)
    .bind(err_str)
    .bind(provider)
    .bind(delivery_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn upsert_repo_state(
    pool: &sqlx::SqlitePool,
    provider: &str,
    repo_key: &str,
    last_push_ref: Option<&str>,
    last_push_sha: Option<&str>,
    last_check_run_sha: Option<&str>,
    last_check_run_status: Option<&str>,
    last_check_run_conclusion: Option<&str>,
) -> Result<(), sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO webhook_repo_states (
                id,
                provider,
                repo_key,
                last_push_ref,
                last_push_sha,
                last_check_run_sha,
                last_check_run_status,
                last_check_run_conclusion
           ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(provider, repo_key) DO UPDATE SET
                last_push_ref = COALESCE(excluded.last_push_ref, last_push_ref),
                last_push_sha = COALESCE(excluded.last_push_sha, last_push_sha),
                last_check_run_sha = COALESCE(excluded.last_check_run_sha, last_check_run_sha),
                last_check_run_status = COALESCE(excluded.last_check_run_status, last_check_run_status),
                last_check_run_conclusion = COALESCE(excluded.last_check_run_conclusion, last_check_run_conclusion),
                updated_at = datetime('now', 'subsec')"#,
    )
    .bind(id)
    .bind(provider)
    .bind(repo_key)
    .bind(last_push_ref)
    .bind(last_push_sha)
    .bind(last_check_run_sha)
    .bind(last_check_run_status)
    .bind(last_check_run_conclusion)
    .execute(pool)
    .await?;
    Ok(())
}

async fn upsert_repo_state_github_push(
    pool: &sqlx::SqlitePool,
    payload: GitHubPushEvent,
) -> Result<(), sqlx::Error> {
    upsert_repo_state(
        pool,
        "github",
        &payload.repository.full_name,
        Some(&payload.git_ref),
        Some(&payload.after),
        None,
        None,
        None,
    )
    .await
}

async fn upsert_repo_state_github_check_run(
    pool: &sqlx::SqlitePool,
    payload: GitHubCheckRunEvent,
) -> Result<(), sqlx::Error> {
    upsert_repo_state(
        pool,
        "github",
        &payload.repository.full_name,
        None,
        None,
        Some(&payload.check_run.head_sha),
        payload.check_run.status.as_deref(),
        payload.check_run.conclusion.as_deref(),
    )
    .await
}

async fn upsert_repo_state_gitlab_push(
    pool: &sqlx::SqlitePool,
    payload: GitLabPushEvent,
) -> Result<(), sqlx::Error> {
    let sha = payload
        .checkout_sha
        .as_deref()
        .or(payload.after.as_deref());
    upsert_repo_state(
        pool,
        "gitlab",
        &payload.project.path_with_namespace,
        Some(&payload.git_ref),
        sha,
        None,
        None,
        None,
    )
    .await
}

fn ok(ack: WebhookAck) -> (StatusCode, ResponseJson<ApiResponse<WebhookAck>>) {
    (StatusCode::OK, ResponseJson(ApiResponse::success(ack)))
}

fn bad_request(msg: &str) -> (StatusCode, ResponseJson<ApiResponse<WebhookAck>>) {
    (StatusCode::BAD_REQUEST, ResponseJson(ApiResponse::error(msg)))
}

fn unauthorized(msg: &str) -> (StatusCode, ResponseJson<ApiResponse<WebhookAck>>) {
    (StatusCode::UNAUTHORIZED, ResponseJson(ApiResponse::error(msg)))
}

fn internal_error(msg: &str) -> (StatusCode, ResponseJson<ApiResponse<WebhookAck>>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        ResponseJson(ApiResponse::error(msg)),
    )
}
