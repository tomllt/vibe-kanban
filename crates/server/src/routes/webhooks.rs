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
use utils::response::ApiResponse;

use crate::DeploymentImpl;

const ENV_GITHUB_SECRET_PRIMARY: &str = "GITHUB_WEBHOOK_SECRET";
const ENV_GITHUB_SECRET_FALLBACK: &str = "GITHUB_APP_WEBHOOK_SECRET";

const ENV_GITLAB_TOKEN_PRIMARY: &str = "GITLAB_WEBHOOK_TOKEN";
const ENV_GITLAB_TOKEN_FALLBACK: &str = "GITLAB_WEBHOOK_SECRET";

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

    let payload_json = String::from_utf8_lossy(&body).to_string();
    let pool = &deployment.db().pool;

    let inserted = match upsert_delivery(
        pool,
        "github",
        &delivery_id,
        &event,
        true,
        &payload_json,
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
            ok(WebhookAck {
                provider: "github",
                event,
                delivery_id,
                status,
                deduped: false,
                updated_merges,
                updated_tasks,
            })
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
    let delivery_id = header_str(&headers, "X-Gitlab-Event-UUID")
        .or_else(|| header_str(&headers, "X-Request-Id"))
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

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

    let payload_json = String::from_utf8_lossy(&body).to_string();
    let pool = &deployment.db().pool;

    let inserted =
        match upsert_delivery(pool, "gitlab", &delivery_id, &event, true, &payload_json).await {
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
            ok(WebhookAck {
                provider: "gitlab",
                event,
                delivery_id,
                status,
                deduped: false,
                updated_merges,
                updated_tasks,
            })
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
        "push" | "check_run" => Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        }),
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
    // GitLab event naming is inconsistent across hook types, so key off payload content
    let payload: GitLabWebhook = serde_json::from_slice(body)?;
    match payload.object_kind.as_str() {
        "merge_request" => process_gitlab_merge_request(deployment, payload).await,
        "push" => Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        }),
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
    payload: GitLabWebhook,
) -> anyhow::Result<ProcessResult> {
    let Some(attrs) = payload.object_attributes else {
        return Ok(ProcessResult {
            status: "ignored",
            updated_merges: 0,
            updated_tasks: 0,
        });
    };

    let next_status = gitlab_mr_status(&attrs);
    let pr_url = attrs.url;
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
            "{}",
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
            "{}",
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
}

#[derive(Debug, Deserialize)]
struct GitHubPullRequestEvent {
    pull_request: GitHubPullRequest,
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
struct GitLabWebhook {
    object_kind: String,
    #[serde(default)]
    object_attributes: Option<GitLabMergeRequestAttributes>,
}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequestAttributes {
    url: String,
    state: String,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    merge_commit_sha: Option<String>,
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
    payload_json: &str,
) -> Result<bool, sqlx::Error> {
    let id = uuid::Uuid::new_v4();

    let result = sqlx::query(
        r#"INSERT INTO webhook_deliveries (
                id, provider, delivery_id, event, signature_valid, status, attempts, payload_json
           ) VALUES (?, ?, ?, ?, ?, 'received', 1, ?)
           ON CONFLICT(provider, delivery_id) DO NOTHING"#,
    )
    .bind(id)
    .bind(provider)
    .bind(delivery_id)
    .bind(event)
    .bind(signature_valid as i64)
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
