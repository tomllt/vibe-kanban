use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool, Type};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, TS, Type, PartialEq, Eq)]
#[sqlx(type_name = "workflow_environment", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum WorkflowEnvironment {
    Staging,
    Prod,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, Type, PartialEq, Eq)]
#[sqlx(type_name = "promotion_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PromotionStatus {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, FromRow)]
pub struct EnvironmentPromotion {
    pub id: Uuid,
    pub task_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub environment: WorkflowEnvironment,
    pub status: PromotionStatus,
    pub target_branch: String,
    pub merge_commit_sha: Option<String>,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EnvironmentPromotion {
    pub async fn create(
        pool: &SqlitePool,
        id: Uuid,
        task_id: Uuid,
        workspace_id: Option<Uuid>,
        environment: WorkflowEnvironment,
        status: PromotionStatus,
        target_branch: &str,
        merge_commit_sha: Option<&str>,
        message: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, EnvironmentPromotion>(
            r#"INSERT INTO environment_promotions (
                id, task_id, workspace_id, environment, status, target_branch, merge_commit_sha, message
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING
              id, task_id, workspace_id, environment, status, target_branch, merge_commit_sha, message, created_at, updated_at"#,
        )
        .bind(id)
        .bind(task_id)
        .bind(workspace_id)
        .bind(environment)
        .bind(status)
        .bind(target_branch)
        .bind(merge_commit_sha)
        .bind(message)
        .fetch_one(pool)
        .await
    }

    /// Returns latest promotion per (task_id, environment).
    pub async fn latest_by_task_ids(
        pool: &SqlitePool,
        task_ids: &[Uuid],
    ) -> Result<Vec<Self>, sqlx::Error> {
        if task_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"SELECT
                 ep.id,
                 ep.task_id,
                 ep.workspace_id,
                 ep.environment,
                 ep.status,
                 ep.target_branch,
                 ep.merge_commit_sha,
                 ep.message,
                 ep.created_at,
                 ep.updated_at
               FROM environment_promotions ep
               JOIN (
                 SELECT task_id, environment, MAX(created_at) AS max_created_at
                 FROM environment_promotions
                 WHERE task_id IN ("#,
        );

        let mut separated = query_builder.separated(", ");
        for task_id in task_ids {
            separated.push_bind(task_id);
        }
        separated.push_unseparated(") GROUP BY task_id, environment ) latest\n");
        query_builder.push(
            "ON ep.task_id = latest.task_id AND ep.environment = latest.environment AND ep.created_at = latest.max_created_at",
        );

        query_builder
            .build_query_as::<EnvironmentPromotion>()
            .fetch_all(pool)
            .await
    }
}
