use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool, Type};
use strum_macros::{Display, EnumString};
use ts_rs::TS;
use uuid::Uuid;

#[derive(
    Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default,
)]
#[sqlx(type_name = "sprint_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum SprintStatus {
    #[default]
    Planned,
    Active,
    Closed,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct Sprint {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub goal: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub status: SprintStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct CreateSprint {
    pub project_id: Uuid,
    pub name: String,
    pub goal: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub status: Option<SprintStatus>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct UpdateSprint {
    pub name: Option<String>,
    pub goal: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub status: Option<SprintStatus>,
}

impl Sprint {
    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            Sprint,
            r#"SELECT
                  id as "id!: Uuid",
                  project_id as "project_id!: Uuid",
                  name,
                  goal,
                  start_date as "start_date: DateTime<Utc>",
                  end_date as "end_date: DateTime<Utc>",
                  status as "status!: SprintStatus",
                  created_at as "created_at!: DateTime<Utc>",
                  updated_at as "updated_at!: DateTime<Utc>"
               FROM sprints
               WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_project_id(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            Sprint,
            r#"SELECT
                  id as "id!: Uuid",
                  project_id as "project_id!: Uuid",
                  name,
                  goal,
                  start_date as "start_date: DateTime<Utc>",
                  end_date as "end_date: DateTime<Utc>",
                  status as "status!: SprintStatus",
                  created_at as "created_at!: DateTime<Utc>",
                  updated_at as "updated_at!: DateTime<Utc>"
               FROM sprints
               WHERE project_id = $1
               ORDER BY created_at DESC"#,
            project_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn create(
        pool: &SqlitePool,
        data: &CreateSprint,
        sprint_id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        let status = data.status.clone().unwrap_or_default();
        sqlx::query_as!(
            Sprint,
            r#"INSERT INTO sprints (id, project_id, name, goal, start_date, end_date, status)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING
                  id as "id!: Uuid",
                  project_id as "project_id!: Uuid",
                  name,
                  goal,
                  start_date as "start_date: DateTime<Utc>",
                  end_date as "end_date: DateTime<Utc>",
                  status as "status!: SprintStatus",
                  created_at as "created_at!: DateTime<Utc>",
                  updated_at as "updated_at!: DateTime<Utc>""#,
            sprint_id,
            data.project_id,
            data.name,
            data.goal,
            data.start_date,
            data.end_date,
            status
        )
        .fetch_one(pool)
        .await
    }

    pub async fn update(
        pool: &SqlitePool,
        existing: &Sprint,
        data: &UpdateSprint,
    ) -> Result<Self, sqlx::Error> {
        let name = data.name.as_ref().unwrap_or(&existing.name);
        let goal = data.goal.clone().or_else(|| existing.goal.clone());
        let start_date = data.start_date.or(existing.start_date);
        let end_date = data.end_date.or(existing.end_date);
        let status = data.status.clone().unwrap_or_else(|| existing.status.clone());

        sqlx::query_as!(
            Sprint,
            r#"UPDATE sprints
               SET name = $2,
                   goal = $3,
                   start_date = $4,
                   end_date = $5,
                   status = $6,
                   updated_at = datetime('now', 'subsec')
               WHERE id = $1
               RETURNING
                  id as "id!: Uuid",
                  project_id as "project_id!: Uuid",
                  name,
                  goal,
                  start_date as "start_date: DateTime<Utc>",
                  end_date as "end_date: DateTime<Utc>",
                  status as "status!: SprintStatus",
                  created_at as "created_at!: DateTime<Utc>",
                  updated_at as "updated_at!: DateTime<Utc>""#,
            existing.id,
            name,
            goal,
            start_date,
            end_date,
            status
        )
        .fetch_one(pool)
        .await
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!("DELETE FROM sprints WHERE id = $1", id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}
