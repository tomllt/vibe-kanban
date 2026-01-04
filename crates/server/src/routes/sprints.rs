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
