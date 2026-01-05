use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;
use uuid::Uuid;

use super::task::TaskStatus;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct TaskStatusEvent {
    pub id: Uuid,
    pub task_id: Uuid,
    pub project_id: Uuid,
    pub status: TaskStatus,
    #[ts(type = "Date")]
    pub created_at: DateTime<Utc>,
}

impl TaskStatusEvent {
    pub async fn list_for_project_up_to(
        pool: &SqlitePool,
        project_id: Uuid,
        up_to: DateTime<Utc>,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskStatusEvent,
            r#"SELECT
                id as "id!: Uuid",
                task_id as "task_id!: Uuid",
                project_id as "project_id!: Uuid",
                status as "status!: TaskStatus",
                created_at as "created_at!: DateTime<Utc>"
            FROM task_status_events
            WHERE project_id = $1 AND created_at <= $2
            ORDER BY created_at ASC"#,
            project_id,
            up_to
        )
        .fetch_all(pool)
        .await
    }

    pub async fn list_for_project_between(
        pool: &SqlitePool,
        project_id: Uuid,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskStatusEvent,
            r#"SELECT
                id as "id!: Uuid",
                task_id as "task_id!: Uuid",
                project_id as "project_id!: Uuid",
                status as "status!: TaskStatus",
                created_at as "created_at!: DateTime<Utc>"
            FROM task_status_events
            WHERE project_id = $1 AND created_at >= $2 AND created_at <= $3
            ORDER BY created_at ASC"#,
            project_id,
            from,
            to
        )
        .fetch_all(pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{CreateTask, Task};

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[tokio::test]
    async fn creates_status_events_via_triggers() {
        let pool = setup_pool().await;

        let project_id = Uuid::new_v4();
        sqlx::query("INSERT INTO projects (id, name) VALUES (?, ?)")
            .bind(project_id)
            .bind("P")
            .execute(&pool)
            .await
            .expect("insert project");

        let task_id = Uuid::new_v4();
        let task = Task::create(
            &pool,
            &CreateTask::from_title_description(project_id, "T1".to_string(), None),
            task_id,
        )
        .await
        .expect("create task");

        let initial: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM task_status_events WHERE task_id = ?")
                .bind(task.id)
                .fetch_one(&pool)
                .await
                .expect("count events");
        assert_eq!(initial, 1);

        Task::update(
            &pool,
            task.id,
            task.project_id,
            task.title.clone(),
            task.description.clone(),
            TaskStatus::InProgress,
            task.parent_workspace_id,
        )
        .await
        .expect("update task");

        let after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM task_status_events WHERE task_id = ?")
                .bind(task.id)
                .fetch_one(&pool)
                .await
                .expect("count events");
        assert_eq!(after, 2);

        let last: TaskStatusEvent = sqlx::query_as(
            r#"SELECT id, task_id, project_id, status, created_at
               FROM task_status_events
               WHERE task_id = ?
               ORDER BY created_at DESC
               LIMIT 1"#,
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("fetch last event");

        assert_eq!(last.status, TaskStatus::InProgress);
        assert_eq!(last.project_id, project_id);
    }
}
