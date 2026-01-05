use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

pub struct ProjectDevEx;

impl ProjectDevEx {
    pub async fn list_agent_turn_timestamps(
        pool: &SqlitePool,
        project_id: Uuid,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT cat.created_at as "created_at!: DateTime<Utc>"
           FROM coding_agent_turns cat
           JOIN execution_processes ep ON ep.id = cat.execution_process_id
           JOIN sessions s ON s.id = ep.session_id
           JOIN workspaces w ON w.id = s.workspace_id
           JOIN tasks t ON t.id = w.task_id
          WHERE t.project_id = $1
            AND cat.created_at >= $2
            AND cat.created_at <= $3
          ORDER BY cat.created_at ASC"#,
            project_id,
            from,
            to
        )
        .fetch_all(pool)
        .await
    }

    pub async fn list_agent_run_timestamps(
        pool: &SqlitePool,
        project_id: Uuid,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT ep.created_at as "created_at!: DateTime<Utc>"
           FROM execution_processes ep
           JOIN sessions s ON s.id = ep.session_id
           JOIN workspaces w ON w.id = s.workspace_id
           JOIN tasks t ON t.id = w.task_id
          WHERE t.project_id = $1
            AND ep.run_reason = 'codingagent'
            AND ep.created_at >= $2
            AND ep.created_at <= $3
          ORDER BY ep.created_at ASC"#,
            project_id,
            from,
            to
        )
        .fetch_all(pool)
        .await
    }

    pub async fn count_tasks_touched(
        pool: &SqlitePool,
        project_id: Uuid,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT COUNT(DISTINCT t.id) as "count!: i64"
           FROM execution_processes ep
           JOIN sessions s ON s.id = ep.session_id
           JOIN workspaces w ON w.id = s.workspace_id
           JOIN tasks t ON t.id = w.task_id
          WHERE t.project_id = $1
            AND ep.run_reason = 'codingagent'
            AND ep.created_at >= $2
            AND ep.created_at <= $3"#,
            project_id,
            from,
            to
        )
        .fetch_one(pool)
        .await
    }
}
