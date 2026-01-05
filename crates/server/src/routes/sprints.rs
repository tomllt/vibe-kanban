use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    middleware::from_fn_with_state,
    response::Json as ResponseJson,
    routing::{get, post},
};
use db::models::{
    sprint::{CreateSprint, Sprint, UpdateSprint},
    task::TaskType,
};
use deployment::Deployment;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ts_rs::TS;
use utils::{api::oauth::LoginStatus, response::ApiResponse};
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError, middleware::load_sprint_middleware};

#[derive(Debug, Deserialize, TS)]
pub struct SprintQuery {
    pub project_id: Uuid,
}

pub async fn get_sprints(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<SprintQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<Sprint>>>, ApiError> {
    let sprints = Sprint::find_by_project_id(&deployment.db().pool, query.project_id).await?;
    Ok(ResponseJson(ApiResponse::success(sprints)))
}

pub async fn get_sprint(
    Extension(sprint): Extension<Sprint>,
) -> Result<ResponseJson<ApiResponse<Sprint>>, ApiError> {
    Ok(ResponseJson(ApiResponse::success(sprint)))
}

pub async fn create_sprint(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateSprint>,
) -> Result<ResponseJson<ApiResponse<Sprint>>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(ApiError::BadRequest("Sprint name cannot be empty".to_string()));
    }

    let sprint = Sprint::create(&deployment.db().pool, &payload, Uuid::new_v4()).await?;

    deployment
        .track_if_analytics_allowed(
            "sprint_created",
            serde_json::json!({
                "sprint_id": sprint.id.to_string(),
                "project_id": sprint.project_id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(sprint)))
}

pub async fn update_sprint(
    Extension(existing): Extension<Sprint>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<UpdateSprint>,
) -> Result<ResponseJson<ApiResponse<Sprint>>, ApiError> {
    if let Some(name) = payload.name.as_ref()
        && name.trim().is_empty()
    {
        return Err(ApiError::BadRequest("Sprint name cannot be empty".to_string()));
    }

    let sprint = Sprint::update(&deployment.db().pool, &existing, &payload).await?;

    deployment
        .track_if_analytics_allowed(
            "sprint_updated",
            serde_json::json!({
                "sprint_id": sprint.id.to_string(),
                "project_id": sprint.project_id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(sprint)))
}

pub async fn delete_sprint(
    Extension(sprint): Extension<Sprint>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let rows = Sprint::delete(&deployment.db().pool, sprint.id).await?;
    if rows == 0 {
        return Err(ApiError::Database(sqlx::Error::RowNotFound));
    }

    deployment
        .track_if_analytics_allowed(
            "sprint_deleted",
            serde_json::json!({
                "sprint_id": sprint.id.to_string(),
                "project_id": sprint.project_id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(())))
}

#[derive(Debug, Deserialize, TS)]
pub struct SprintPlanningTaskIds {
    pub task_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, TS)]
pub struct SprintPlanningUpdateResponse {
    pub updated_count: u64,
}

fn format_uuid_list(ids: &[Uuid], max: usize) -> String {
    if ids.is_empty() {
        return "[]".to_string();
    }
    let shown = ids
        .iter()
        .take(max)
        .map(|id| id.to_string())
        .collect::<Vec<_>>();
    if ids.len() > max {
        format!("[{}, … +{} more]", shown.join(", "), ids.len() - max)
    } else {
        format!("[{}]", shown.join(", "))
    }
}

async fn validate_task_ids_belong_to_project(
    pool: &SqlitePool,
    project_id: Uuid,
    task_ids: &[Uuid],
) -> Result<(), ApiError> {
    use std::collections::HashSet;

    if task_ids.is_empty() {
        return Ok(());
    }

    let unique_ids: Vec<Uuid> = task_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut query_builder = sqlx::QueryBuilder::new(
        "SELECT id as id, project_id as project_id FROM tasks WHERE id IN (",
    );
    let mut separated = query_builder.separated(", ");
    for id in &unique_ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");

    let rows: Vec<(Uuid, Uuid)> = query_builder.build_query_as().fetch_all(pool).await?;

    if rows.len() != unique_ids.len() {
        let found: HashSet<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let missing: Vec<Uuid> = unique_ids
            .iter()
            .copied()
            .filter(|id| !found.contains(id))
            .collect();
        return Err(ApiError::BadRequest(format!(
            "Some task_ids were not found: {}",
            format_uuid_list(&missing, 10)
        )));
    }

    if let Some((bad_id, bad_project_id)) = rows
        .iter()
        .find(|(_, row_project_id)| *row_project_id != project_id)
        .copied()
    {
        return Err(ApiError::BadRequest(format!(
            "Task {} does not belong to sprint project {} (found project {})",
            bad_id, project_id, bad_project_id
        )));
    }

    Ok(())
}

async fn ensure_shared_task_auth_for_any(
    deployment: &DeploymentImpl,
    pool: &SqlitePool,
    task_ids: &[Uuid],
) -> Result<(), ApiError> {
    if task_ids.is_empty() {
        return Ok(());
    }

    let mut query_builder =
        sqlx::QueryBuilder::new("SELECT COUNT(*) as count FROM tasks WHERE shared_task_id IS NOT NULL AND id IN (");
    let mut separated = query_builder.separated(", ");
    for id in task_ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");

    let (count,): (i64,) = query_builder.build_query_as().fetch_one(pool).await?;
    if count > 0 {
        match deployment.get_login_status().await {
            LoginStatus::LoggedIn { .. } => Ok(()),
            LoginStatus::LoggedOut => Err(services::services::share::ShareError::MissingAuth.into()),
        }
    } else {
        Ok(())
    }
}

async fn validate_no_epics(pool: &SqlitePool, task_ids: &[Uuid]) -> Result<(), ApiError> {
    if task_ids.is_empty() {
        return Ok(());
    }

    let mut query_builder =
        sqlx::QueryBuilder::new("SELECT id as id, task_type as task_type FROM tasks WHERE id IN (");
    let mut separated = query_builder.separated(", ");
    for id in task_ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");

    let rows: Vec<(Uuid, TaskType)> = query_builder.build_query_as().fetch_all(pool).await?;
    if rows.iter().any(|(_, t)| *t == TaskType::Epic) {
        return Err(ApiError::BadRequest(
            "Epic tasks cannot be assigned to a sprint".to_string(),
        ));
    }
    Ok(())
}

pub async fn assign_to_sprint(
    Extension(sprint): Extension<Sprint>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<SprintPlanningTaskIds>,
) -> Result<ResponseJson<ApiResponse<SprintPlanningUpdateResponse>>, ApiError> {
    if payload.task_ids.is_empty() {
        return Ok(ResponseJson(ApiResponse::success(
            SprintPlanningUpdateResponse { updated_count: 0 },
        )));
    }

    validate_task_ids_belong_to_project(
        &deployment.db().pool,
        sprint.project_id,
        &payload.task_ids,
    )
    .await?;
    ensure_shared_task_auth_for_any(&deployment, &deployment.db().pool, &payload.task_ids).await?;
    validate_no_epics(&deployment.db().pool, &payload.task_ids).await?;

    let mut query_builder = sqlx::QueryBuilder::new(
        "UPDATE tasks SET sprint_id = ",
    );
    query_builder
        .push_bind(sprint.id)
        .push(", updated_at = datetime('now', 'subsec') WHERE project_id = ")
        .push_bind(sprint.project_id)
        .push(" AND id IN (");

    let mut separated = query_builder.separated(", ");
    for id in &payload.task_ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");

    let result = query_builder.build().execute(&deployment.db().pool).await?;

    Ok(ResponseJson(ApiResponse::success(
        SprintPlanningUpdateResponse {
            updated_count: result.rows_affected(),
        },
    )))
}

pub async fn unassign_from_sprint(
    Extension(sprint): Extension<Sprint>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<SprintPlanningTaskIds>,
) -> Result<ResponseJson<ApiResponse<SprintPlanningUpdateResponse>>, ApiError> {
    if payload.task_ids.is_empty() {
        return Ok(ResponseJson(ApiResponse::success(
            SprintPlanningUpdateResponse { updated_count: 0 },
        )));
    }

    validate_task_ids_belong_to_project(
        &deployment.db().pool,
        sprint.project_id,
        &payload.task_ids,
    )
    .await?;
    ensure_shared_task_auth_for_any(&deployment, &deployment.db().pool, &payload.task_ids).await?;

    let mut query_builder = sqlx::QueryBuilder::new(
        "UPDATE tasks SET sprint_id = NULL, updated_at = datetime('now', 'subsec') WHERE project_id = ",
    );
    query_builder
        .push_bind(sprint.project_id)
        .push(" AND sprint_id = ")
        .push_bind(sprint.id)
        .push(" AND id IN (");

    let mut separated = query_builder.separated(", ");
    for id in &payload.task_ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");

    let result = query_builder.build().execute(&deployment.db().pool).await?;

    Ok(ResponseJson(ApiResponse::success(
        SprintPlanningUpdateResponse {
            updated_count: result.rows_affected(),
        },
    )))
}

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let sprint_actions = Router::new()
        .route("/", get(get_sprint).put(update_sprint).delete(delete_sprint))
        .route("/assign", post(assign_to_sprint))
        .route("/unassign", post(unassign_from_sprint))
        .layer(from_fn_with_state(
            deployment.clone(),
            load_sprint_middleware,
        ));

    let inner = Router::new()
        .route("/", get(get_sprints).post(create_sprint))
        .nest("/{sprint_id}", sprint_actions);

    Router::new().nest("/sprints", inner)
}

#[cfg(test)]
mod tests {
    use db::models::{
        project::{CreateProject, Project},
        task::{CreateTask, Task, TaskStatus, TaskType},
    };
    use sqlx::SqlitePool;
    use uuid::Uuid;

    use super::validate_task_ids_belong_to_project;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        pool
    }

    async fn create_project(pool: &SqlitePool, name: &str) -> Project {
        let project_id = Uuid::new_v4();
        Project::create(
            pool,
            &CreateProject {
                name: name.to_string(),
                repositories: vec![],
            },
            project_id,
        )
        .await
        .unwrap()
    }

    async fn create_task(pool: &SqlitePool, project_id: Uuid) -> Task {
        Task::create(
            pool,
            &CreateTask {
                project_id,
                title: "Task".to_string(),
                description: None,
                status: Some(TaskStatus::Todo),
                sprint_id: None,
                task_type: Some(TaskType::Task),
                epic_id: None,
                parent_task_id: None,
                story_points: None,
                parent_workspace_id: None,
                image_ids: None,
                shared_task_id: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn validates_task_ids_belong_to_project_ok() {
        let pool = setup_pool().await;
        let project = create_project(&pool, "P1").await;
        let task = create_task(&pool, project.id).await;

        validate_task_ids_belong_to_project(&pool, project.id, &[task.id])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn validates_task_ids_belong_to_project_errors_for_missing() {
        let pool = setup_pool().await;
        let project = create_project(&pool, "P1").await;

        let err = validate_task_ids_belong_to_project(&pool, project.id, &[Uuid::new_v4()])
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Some task_ids were not found"));
    }

    #[tokio::test]
    async fn validates_task_ids_belong_to_project_errors_for_wrong_project() {
        let pool = setup_pool().await;
        let sprint_project = create_project(&pool, "Sprint").await;
        let other_project = create_project(&pool, "Other").await;
        let task = create_task(&pool, other_project.id).await;

        let err =
            validate_task_ids_belong_to_project(&pool, sprint_project.id, &[task.id])
                .await
                .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("does not belong to sprint project"));
    }
}
