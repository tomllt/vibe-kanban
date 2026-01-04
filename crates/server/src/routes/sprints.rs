use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue},
    middleware::from_fn_with_state,
    response::{IntoResponse, Json as ResponseJson},
    routing::{get, put},
};
use chrono::{DateTime, Utc};
use db::models::{
    project::Project,
    sprint::{CreateSprint, Sprint, UpdateSprint},
    task::Task,
    workspace::Workspace,
};
use deployment::Deployment;
use serde::Deserialize;
use ts_rs::TS;
use utils::response::ApiResponse;

use crate::{
    DeploymentImpl,
    error::ApiError,
    middleware::load_sprint_middleware,
    release_notes::{build_release_notes_response, build_task_item},
};

#[derive(Debug, Deserialize, TS)]
pub struct CreateSprintRequest {
    pub name: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, TS)]
pub struct UpdateSprintRequest {
    pub name: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, TS)]
pub struct ReleaseNotesQuery {
    #[serde(default)]
    pub download: Option<bool>,
}

pub async fn list_sprints(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<Sprint>>>, ApiError> {
    let sprints = Sprint::find_by_project_id(&deployment.db().pool, project.id).await?;
    Ok(ResponseJson(ApiResponse::success(sprints)))
}

pub async fn create_sprint(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateSprintRequest>,
) -> Result<ResponseJson<ApiResponse<Sprint>>, ApiError> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("Sprint name cannot be empty".to_string()));
    }
    if payload.end_at <= payload.start_at {
        return Err(ApiError::BadRequest(
            "Sprint end_at must be after start_at".to_string(),
        ));
    }

    let sprint = Sprint::create(
        &deployment.db().pool,
        &CreateSprint {
            project_id: project.id,
            name,
            start_at: payload.start_at,
            end_at: payload.end_at,
        },
    )
    .await?;

    Ok(ResponseJson(ApiResponse::success(sprint)))
}

pub async fn update_sprint(
    Extension(sprint): Extension<Sprint>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<UpdateSprintRequest>,
) -> Result<ResponseJson<ApiResponse<Sprint>>, ApiError> {
    let name = payload.name.map(|n| n.trim().to_string());
    if name.as_deref().is_some_and(|n| n.is_empty()) {
        return Err(ApiError::BadRequest("Sprint name cannot be empty".to_string()));
    }

    let start_at = payload.start_at.unwrap_or(sprint.start_at);
    let end_at = payload.end_at.unwrap_or(sprint.end_at);
    if end_at <= start_at {
        return Err(ApiError::BadRequest(
            "Sprint end_at must be after start_at".to_string(),
        ));
    }

    let updated = Sprint::update(
        &deployment.db().pool,
        sprint.id,
        &UpdateSprint {
            name,
            start_at: payload.start_at,
            end_at: payload.end_at,
        },
    )
    .await?;

    Ok(ResponseJson(ApiResponse::success(updated)))
}

pub async fn delete_sprint(
    Extension(sprint): Extension<Sprint>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let mut tx = deployment.db().pool.begin().await?;
    let rows_affected = Sprint::delete(&mut *tx, sprint.id).await?;
    tx.commit().await?;

    if rows_affected == 0 {
        return Err(ApiError::Database(sqlx::Error::RowNotFound));
    }
    Ok(ResponseJson(ApiResponse::success(())))
}

pub async fn get_release_notes(
    Extension(project): Extension<Project>,
    Extension(sprint): Extension<Sprint>,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ReleaseNotesQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let pool = &deployment.db().pool;

    let tasks =
        Task::find_done_by_project_and_range(pool, project.id, sprint.start_at, sprint.end_at)
            .await?;

    let mut items = Vec::with_capacity(tasks.len());
    for task in tasks {
        let workspaces = Workspace::fetch_all(pool, Some(task.id))
            .await
            .map_err(ApiError::Workspace)?;

        let mut merges = Vec::new();
        for ws in workspaces {
            let mut ws_merges = db::models::merge::Merge::find_by_workspace_id(pool, ws.id).await?;
            ws_merges.retain(|m| merge_created_at_in_range(m, sprint.start_at, sprint.end_at));
            merges.extend(ws_merges);
        }

        items.push(build_task_item(task, merges));
    }

    let response = build_release_notes_response(sprint, items);

    if query.download.unwrap_or(false) {
        let filename = format!(
            "release-notes-{}.md",
            slug_filename(&response.sprint.name)
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/markdown; charset=utf-8"),
        );
        headers.insert(
            axum::http::header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
        );

        return Ok((headers, response.markdown).into_response());
    }

    let body: ApiResponse<crate::release_notes::ReleaseNotesResponse> = ApiResponse::success(response);
    Ok(ResponseJson(body).into_response())
}

fn merge_created_at_in_range(
    merge: &db::models::merge::Merge,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> bool {
    let created_at = match merge {
        db::models::merge::Merge::Direct(m) => m.created_at,
        db::models::merge::Merge::Pr(m) => m.created_at,
    };
    created_at >= start && created_at < end
}

fn slug_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == ' ' || ch == '-' || ch == '_' {
            if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        "sprint".to_string()
    } else {
        slug
    }
}

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let sprint_id_router = Router::new()
        .route("/", put(update_sprint).delete(delete_sprint))
        .route("/release-notes", get(get_release_notes))
        .layer(from_fn_with_state(
            deployment.clone(),
            load_sprint_middleware,
        ));

    Router::new()
        .route("/", get(list_sprints).post(create_sprint))
        .nest("/{sprint_id}", sprint_id_router)
}
