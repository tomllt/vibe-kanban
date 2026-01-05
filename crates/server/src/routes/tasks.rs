use std::path::PathBuf;

use anyhow;
use axum::{
    Extension, Json, Router,
    extract::{
        Query, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    middleware::from_fn_with_state,
    response::{IntoResponse, Json as ResponseJson},
    routing::{delete, get, post, put},
};
use db::models::{
    environment_promotion::{EnvironmentPromotion, PromotionStatus, WorkflowEnvironment},
    image::TaskImage,
    project::{Project, ProjectError},
    repo::Repo,
    sprint::Sprint,
    task::{CreateTask, Task, TaskType, TaskWithAttemptStatus, UpdateTask, UpdateTaskData},
    workspace::{CreateWorkspace, Workspace},
    workspace_repo::{CreateWorkspaceRepo, WorkspaceRepo},
};
use deployment::Deployment;
use executors::backlog_groomer::{BacklogGroomer, BacklogGroomerError, BacklogGroomingDraft};
use executors::profile::ExecutorProfileId;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use services::services::{
    container::ContainerService,
    git::GitBranchKind,
    share::ShareError,
    workspace_manager::WorkspaceManager,
};
use sqlx::Error as SqlxError;
use ts_rs::TS;
use utils::{api::oauth::LoginStatus, response::ApiResponse};
use uuid::Uuid;

use crate::{
    DeploymentImpl, error::ApiError, middleware::load_task_middleware,
    routes::task_attempts::WorkspaceRepoInput,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskQuery {
    pub project_id: Uuid,
}

pub async fn get_tasks(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<TaskQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<TaskWithAttemptStatus>>>, ApiError> {
    let tasks =
        Task::find_by_project_id_with_attempt_status(&deployment.db().pool, query.project_id)
            .await?;

    Ok(ResponseJson(ApiResponse::success(tasks)))
}

#[derive(Debug, Deserialize, TS)]
pub struct BacklogQuery {
    pub project_id: Uuid,
    #[serde(default)]
    pub include_done: bool,
    #[serde(default)]
    pub include_cancelled: bool,
    #[serde(default)]
    pub include_in_sprint: bool,
}

pub async fn get_backlog(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<BacklogQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<TaskWithAttemptStatus>>>, ApiError> {
    let mut tasks =
        Task::find_by_project_id_with_attempt_status(&deployment.db().pool, query.project_id)
            .await?;

    if !query.include_in_sprint {
        tasks.retain(|t| t.sprint_id.is_none());
    }

    if !query.include_done {
        tasks.retain(|t| t.status != db::models::task::TaskStatus::Done);
    }

    if !query.include_cancelled {
        tasks.retain(|t| t.status != db::models::task::TaskStatus::Cancelled);
    }

    tasks.sort_by(|a, b| {
        // "epic" first, then feature, story, task.
        let rank = |t: &TaskWithAttemptStatus| match t.task_type {
            TaskType::Epic => 0,
            TaskType::Feature => 1,
            TaskType::Story => 2,
            TaskType::Task => 3,
        };
        rank(a).cmp(&rank(b)).then_with(|| b.created_at.cmp(&a.created_at))
    });

    Ok(ResponseJson(ApiResponse::success(tasks)))
}

pub async fn stream_tasks_ws(
    ws: WebSocketUpgrade,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<TaskQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_tasks_ws(socket, deployment, query.project_id).await {
            tracing::warn!("tasks WS closed: {}", e);
        }
    })
}

async fn handle_tasks_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    project_id: Uuid,
) -> anyhow::Result<()> {
    // Get the raw stream and convert LogMsg to WebSocket messages
    let mut stream = deployment
        .events()
        .stream_tasks_raw(project_id)
        .await?
        .map_ok(|msg| msg.to_ws_message_unchecked());

    // Split socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();

    // Drain (and ignore) any client->server messages so pings/pongs work
    tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });

    // Forward server messages
    while let Some(item) = stream.next().await {
        match item {
            Ok(msg) => {
                if sender.send(msg).await.is_err() {
                    break; // client disconnected
                }
            }
            Err(e) => {
                tracing::error!("stream error: {}", e);
                break;
            }
        }
    }
    Ok(())
}

pub async fn get_task(
    Extension(task): Extension<Task>,
    State(_deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Task>>, ApiError> {
    Ok(ResponseJson(ApiResponse::success(task)))
}

pub async fn create_task(
    State(deployment): State<DeploymentImpl>,
    Json(mut payload): Json<CreateTask>,
) -> Result<ResponseJson<ApiResponse<Task>>, ApiError> {
    let id = Uuid::new_v4();

    tracing::debug!(
        "Creating task '{}' in project {}",
        payload.title,
        payload.project_id
    );

    let task_type = payload.task_type.clone().unwrap_or_default();
    let (sprint_id, epic_id, parent_task_id, story_points) =
        validate_and_normalize_agile_fields(
            &deployment.db().pool,
            payload.project_id,
            task_type.clone(),
            payload.sprint_id,
            payload.epic_id,
            payload.parent_task_id,
            payload.story_points,
        )
        .await?;

    payload.sprint_id = sprint_id;
    payload.task_type = Some(task_type);
    payload.epic_id = epic_id;
    payload.parent_task_id = parent_task_id;
    payload.story_points = story_points;

    let task = Task::create(&deployment.db().pool, &payload, id).await?;

    if let Some(image_ids) = &payload.image_ids {
        TaskImage::associate_many_dedup(&deployment.db().pool, task.id, image_ids).await?;
    }

    deployment
        .track_if_analytics_allowed(
            "task_created",
            serde_json::json!({
            "task_id": task.id.to_string(),
            "project_id": payload.project_id,
            "has_description": task.description.is_some(),
            "has_images": payload.image_ids.is_some(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(task)))
}

#[derive(Debug, Deserialize, TS)]
pub struct CreateAndStartTaskRequest {
    pub task: CreateTask,
    pub executor_profile_id: ExecutorProfileId,
    pub repos: Vec<WorkspaceRepoInput>,
    #[serde(default)]
    #[ts(optional)]
    pub branch_kind: Option<GitBranchKind>,
}

pub async fn create_task_and_start(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateAndStartTaskRequest>,
) -> Result<ResponseJson<ApiResponse<TaskWithAttemptStatus>>, ApiError> {
    let CreateAndStartTaskRequest {
        task,
        executor_profile_id,
        repos,
    } = payload;

    if repos.is_empty() {
        return Err(ApiError::BadRequest(
            "At least one repository is required".to_string(),
        ));
    }

    let pool = &deployment.db().pool;

    let task_id = Uuid::new_v4();
    let mut task_payload = task;
    let task_type = task_payload.task_type.clone().unwrap_or_default();
    let (sprint_id, epic_id, parent_task_id, story_points) =
        validate_and_normalize_agile_fields(
            pool,
            task_payload.project_id,
            task_type.clone(),
            task_payload.sprint_id,
            task_payload.epic_id,
            task_payload.parent_task_id,
            task_payload.story_points,
        )
        .await?;
    task_payload.sprint_id = sprint_id;
    task_payload.task_type = Some(task_type);
    task_payload.epic_id = epic_id;
    task_payload.parent_task_id = parent_task_id;
    task_payload.story_points = story_points;

    let task = Task::create(pool, &task_payload, task_id).await?;

    if let Some(image_ids) = &task_payload.image_ids {
        TaskImage::associate_many_dedup(pool, task.id, image_ids).await?;
    }

    deployment
        .track_if_analytics_allowed(
            "task_created",
            serde_json::json!({
                "task_id": task.id.to_string(),
                "project_id": task.project_id,
                "has_description": task.description.is_some(),
                "has_images": task_payload.image_ids.is_some(),
            }),
        )
        .await;

    let project = Project::find_by_id(pool, task.project_id)
        .await?
        .ok_or(ProjectError::ProjectNotFound)?;

    let attempt_id = Uuid::new_v4();
    let branch_kind = payload.branch_kind.unwrap_or(GitBranchKind::Feature);
    let git_branch_name = deployment
        .container()
        .git_branch_from_workspace_with_kind(&attempt_id, &task.title, branch_kind)
        .await;

    let agent_working_dir = project
        .default_agent_working_dir
        .as_ref()
        .filter(|dir: &&String| !dir.is_empty())
        .cloned();

    let workspace = Workspace::create(
        pool,
        &CreateWorkspace {
            branch: git_branch_name,
            agent_working_dir,
        },
        attempt_id,
        task.id,
    )
    .await?;

    let workspace_repos: Vec<CreateWorkspaceRepo> = repos
        .iter()
        .map(|r| CreateWorkspaceRepo {
            repo_id: r.repo_id,
            target_branch: r.target_branch.clone(),
        })
        .collect();
    WorkspaceRepo::create_many(&deployment.db().pool, workspace.id, &workspace_repos).await?;

    let is_attempt_running = deployment
        .container()
        .start_workspace(&workspace, executor_profile_id.clone())
        .await
        .inspect_err(|err| tracing::error!("Failed to start task attempt: {}", err))
        .is_ok();
    deployment
        .track_if_analytics_allowed(
            "task_attempt_started",
            serde_json::json!({
                "task_id": task.id.to_string(),
                "executor": &executor_profile_id.executor,
                "variant": &executor_profile_id.variant,
                "workspace_id": workspace.id.to_string(),
            }),
        )
        .await;

    let task = Task::find_by_id(pool, task.id)
        .await?
        .ok_or(ApiError::Database(SqlxError::RowNotFound))?;

    tracing::info!("Started attempt for task {}", task.id);
    Ok(ResponseJson(ApiResponse::success(TaskWithAttemptStatus {
        task,
        has_in_progress_attempt: is_attempt_running,
        last_attempt_failed: false,
        executor: executor_profile_id.executor.to_string(),
    })))
}

pub async fn update_task(
    Extension(existing_task): Extension<Task>,
    State(deployment): State<DeploymentImpl>,

    Json(payload): Json<UpdateTask>,
) -> Result<ResponseJson<ApiResponse<Task>>, ApiError> {
    ensure_shared_task_auth(&existing_task, &deployment).await?;

    let old_status = existing_task.status.clone();

    // Use existing values if not provided in update
    let title = payload.title.unwrap_or(existing_task.title);
    let description = match payload.description {
        Some(s) if s.trim().is_empty() => None, // Empty string = clear description
        Some(s) => Some(s),                     // Non-empty string = update description
        None => existing_task.description,      // Field omitted = keep existing
    };
    let status = payload.status.unwrap_or(existing_task.status);
    let sprint_id = payload.sprint_id.unwrap_or(existing_task.sprint_id);
    let task_type = payload
        .task_type
        .unwrap_or_else(|| existing_task.task_type.clone());
    let epic_id = payload.epic_id.unwrap_or(existing_task.epic_id);
    let parent_task_id = payload.parent_task_id.unwrap_or(existing_task.parent_task_id);
    let story_points = payload.story_points.unwrap_or(existing_task.story_points);
    let parent_workspace_id = payload
        .parent_workspace_id
        .unwrap_or(existing_task.parent_workspace_id);

    let (sprint_id, epic_id, parent_task_id, story_points) = validate_and_normalize_agile_fields(
        &deployment.db().pool,
        existing_task.project_id,
        task_type.clone(),
        sprint_id,
        epic_id,
        parent_task_id,
        story_points,
    )
    .await?;

    // If moving into Staging/Prod, optionally run workflow promotion first.
    if old_status != status {
        let workflow = deployment.config().read().await.workflow_automation.clone();
        if workflow.enabled {
            let env = match status {
                db::models::task::TaskStatus::InReview => Some(WorkflowEnvironment::Staging),
                db::models::task::TaskStatus::Done => Some(WorkflowEnvironment::Prod),
                _ => None,
            };

            if let Some(environment) = env {
                promote_task_to_environment(
                    &deployment,
                    existing_task.id,
                    &title,
                    description.as_deref(),
                    old_status.clone(),
                    environment,
                    workflow,
                )
                .await?;
            }
        }
    }

    let task = Task::update(
        &deployment.db().pool,
        existing_task.id,
        existing_task.project_id,
        UpdateTaskData {
            title,
            description,
            status,
            sprint_id,
            task_type,
            epic_id,
            parent_task_id,
            story_points,
            parent_workspace_id,
        },
    )
    .await?;

    if let Some(image_ids) = &payload.image_ids {
        TaskImage::delete_by_task_id(&deployment.db().pool, task.id).await?;
        TaskImage::associate_many_dedup(&deployment.db().pool, task.id, image_ids).await?;
    }

    // If task has been shared, broadcast update
    if task.shared_task_id.is_some() {
        let Ok(publisher) = deployment.share_publisher() else {
            return Err(ShareError::MissingConfig("share publisher unavailable").into());
        };
        publisher.update_shared_task(&task).await?;
    }

    Ok(ResponseJson(ApiResponse::success(task)))
}

async fn validate_and_normalize_agile_fields(
    pool: &sqlx::SqlitePool,
    project_id: Uuid,
    task_type: TaskType,
    sprint_id: Option<Uuid>,
    epic_id: Option<Uuid>,
    parent_task_id: Option<Uuid>,
    story_points: Option<i32>,
) -> Result<(Option<Uuid>, Option<Uuid>, Option<Uuid>, Option<i32>), ApiError> {
    if let Some(points) = story_points
        && points < 0
    {
        return Err(ApiError::BadRequest(
            "story_points must be a non-negative integer".to_string(),
        ));
    }

    if matches!(task_type, TaskType::Epic) {
        if sprint_id.is_some() {
            return Err(ApiError::BadRequest(
                "Epic tasks cannot be assigned to a sprint".to_string(),
            ));
        }
        if epic_id.is_some() || parent_task_id.is_some() {
            return Err(ApiError::BadRequest(
                "Epic tasks cannot have epic_id/parent_task_id set".to_string(),
            ));
        }
        if story_points.is_some() {
            return Err(ApiError::BadRequest(
                "Epic tasks cannot have story_points set".to_string(),
            ));
        }
        return Ok((None, None, None, None));
    }

    if matches!(task_type, TaskType::Feature) && story_points.is_some() {
        return Err(ApiError::BadRequest(
            "Feature tasks cannot have story_points set".to_string(),
        ));
    }

    if let Some(sprint_id) = sprint_id {
        let sprint = Sprint::find_by_id(pool, sprint_id)
            .await?
            .ok_or_else(|| ApiError::BadRequest("Sprint not found".to_string()))?;

        if sprint.project_id != project_id {
            return Err(ApiError::BadRequest(
                "Sprint does not belong to the task's project".to_string(),
            ));
        }
    }

    let mut normalized_epic_id = epic_id;
    let normalized_parent_task_id = parent_task_id;

    if matches!(task_type, TaskType::Feature | TaskType::Story) && normalized_parent_task_id.is_none()
    {
        return Err(ApiError::BadRequest(
            "Feature and Story tasks must have parent_task_id set".to_string(),
        ));
    }

    if let Some(parent_id) = normalized_parent_task_id {
        let parent = Task::find_by_id(pool, parent_id)
            .await?
            .ok_or_else(|| ApiError::BadRequest("parent_task_id not found".to_string()))?;

        if parent.project_id != project_id {
            return Err(ApiError::BadRequest(
                "parent_task_id does not belong to the task's project".to_string(),
            ));
        }

        match task_type {
            TaskType::Feature => {
                if parent.task_type != TaskType::Epic {
                    return Err(ApiError::BadRequest(
                        "Feature parent_task_id must reference an Epic".to_string(),
                    ));
                }
            }
            TaskType::Story => {
                if parent.task_type != TaskType::Feature {
                    return Err(ApiError::BadRequest(
                        "Story parent_task_id must reference a Feature".to_string(),
                    ));
                }
            }
            TaskType::Task => {
                if !matches!(parent.task_type, TaskType::Story | TaskType::Feature) {
                    return Err(ApiError::BadRequest(
                        "Task parent_task_id must reference a Story or Feature".to_string(),
                    ));
                }
            }
            TaskType::Epic => {}
        }

        let parent_implied_epic_id = match parent.task_type {
            TaskType::Epic => Some(parent.id),
            _ => parent.epic_id,
        };

        if normalized_epic_id.is_none() {
            normalized_epic_id = parent_implied_epic_id;
        } else if parent_implied_epic_id.is_some() && normalized_epic_id != parent_implied_epic_id {
            return Err(ApiError::BadRequest(
                "epic_id must match the parent task's epic".to_string(),
            ));
        }
    }

    if let Some(epic_id) = normalized_epic_id {
        let epic = Task::find_by_id(pool, epic_id)
            .await?
            .ok_or_else(|| ApiError::BadRequest("epic_id not found".to_string()))?;

        if epic.project_id != project_id {
            return Err(ApiError::BadRequest(
                "epic_id does not belong to the task's project".to_string(),
            ));
        }
        if epic.task_type != TaskType::Epic {
            return Err(ApiError::BadRequest(
                "epic_id must reference a task with type 'epic'".to_string(),
            ));
        }
    }

    Ok((sprint_id, normalized_epic_id, normalized_parent_task_id, story_points))
}

async fn ensure_shared_task_auth(
    existing_task: &Task,
    deployment: &local_deployment::LocalDeployment,
) -> Result<(), ApiError> {
    if existing_task.shared_task_id.is_some() {
        match deployment.get_login_status().await {
            LoginStatus::LoggedIn { .. } => return Ok(()),
            LoginStatus::LoggedOut => {
                return Err(ShareError::MissingAuth.into());
            }
        }
    }
    Ok(())
}

pub async fn delete_task(
    Extension(task): Extension<Task>,
    State(deployment): State<DeploymentImpl>,
) -> Result<(StatusCode, ResponseJson<ApiResponse<()>>), ApiError> {
    ensure_shared_task_auth(&task, &deployment).await?;

    // Validate no running execution processes
    if deployment
        .container()
        .has_running_processes(task.id)
        .await?
    {
        return Err(ApiError::Conflict("Task has running execution processes. Please wait for them to complete or stop them first.".to_string()));
    }

    let pool = &deployment.db().pool;

    // Gather task attempts data needed for background cleanup
    let attempts = Workspace::fetch_all(pool, Some(task.id))
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch task attempts for task {}: {}", task.id, e);
            ApiError::Workspace(e)
        })?;

    let repositories = WorkspaceRepo::find_unique_repos_for_task(pool, task.id).await?;

    // Collect workspace directories that need cleanup
    let workspace_dirs: Vec<PathBuf> = attempts
        .iter()
        .filter_map(|attempt| attempt.container_ref.as_ref().map(PathBuf::from))
        .collect();

    if let Some(shared_task_id) = task.shared_task_id {
        let Ok(publisher) = deployment.share_publisher() else {
            return Err(ShareError::MissingConfig("share publisher unavailable").into());
        };
        publisher.delete_shared_task(shared_task_id).await?;
    }

    // Use a transaction to ensure atomicity: either all operations succeed or all are rolled back
    let mut tx = pool.begin().await?;

    // Nullify parent_workspace_id for all child tasks before deletion
    // This breaks parent-child relationships to avoid foreign key constraint violations
    let mut total_children_affected = 0u64;
    for attempt in &attempts {
        let children_affected =
            Task::nullify_children_by_workspace_id(&mut *tx, attempt.id).await?;
        total_children_affected += children_affected;
    }

    // Delete task from database (FK CASCADE will handle task_attempts)
    let rows_affected = Task::delete(&mut *tx, task.id).await?;

    if rows_affected == 0 {
        return Err(ApiError::Database(SqlxError::RowNotFound));
    }

    // Commit the transaction - if this fails, all changes are rolled back
    tx.commit().await?;

    if total_children_affected > 0 {
        tracing::info!(
            "Nullified {} child task references before deleting task {}",
            total_children_affected,
            task.id
        );
    }

    deployment
        .track_if_analytics_allowed(
            "task_deleted",
            serde_json::json!({
                "task_id": task.id.to_string(),
                "project_id": task.project_id.to_string(),
                "attempt_count": attempts.len(),
            }),
        )
        .await;

    let task_id = task.id;
    let pool = pool.clone();
    tokio::spawn(async move {
        tracing::info!(
            "Starting background cleanup for task {} ({} workspaces, {} repos)",
            task_id,
            workspace_dirs.len(),
            repositories.len()
        );

        for workspace_dir in &workspace_dirs {
            if let Err(e) = WorkspaceManager::cleanup_workspace(workspace_dir, &repositories).await
            {
                tracing::error!(
                    "Background workspace cleanup failed for task {} at {}: {}",
                    task_id,
                    workspace_dir.display(),
                    e
                );
            }
        }

        match Repo::delete_orphaned(&pool).await {
            Ok(count) if count > 0 => {
                tracing::info!("Deleted {} orphaned repo records", count);
            }
            Err(e) => {
                tracing::error!("Failed to delete orphaned repos: {}", e);
            }
            _ => {}
        }

        tracing::info!("Background cleanup completed for task {}", task_id);
    });

    // Return 202 Accepted to indicate deletion was scheduled
    Ok((StatusCode::ACCEPTED, ResponseJson(ApiResponse::success(()))))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ShareTaskResponse {
    pub shared_task_id: Uuid,
}

pub async fn share_task(
    Extension(task): Extension<Task>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<ShareTaskResponse>>, ApiError> {
    let Ok(publisher) = deployment.share_publisher() else {
        return Err(ShareError::MissingConfig("share publisher unavailable").into());
    };
    let profile = deployment
        .auth_context()
        .cached_profile()
        .await
        .ok_or(ShareError::MissingAuth)?;
    let shared_task_id = publisher.share_task(task.id, profile.user_id).await?;

    let props = serde_json::json!({
        "task_id": task.id,
        "shared_task_id": shared_task_id,
    });
    deployment
        .track_if_analytics_allowed("start_sharing_task", props)
        .await;

    Ok(ResponseJson(ApiResponse::success(ShareTaskResponse {
        shared_task_id,
    })))
}

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let task_actions_router = Router::new()
        .route("/", put(update_task))
        .route("/", delete(delete_task))
        .route("/share", post(share_task))
        .route("/backlog-grooming", get(get_backlog_grooming_draft))
        .route("/backlog-grooming/generate", post(generate_backlog_grooming))
        .route("/backlog-grooming/apply", post(apply_backlog_grooming));

    let task_id_router = Router::new()
        .route("/", get(get_task))
        .merge(task_actions_router)
        .layer(from_fn_with_state(deployment.clone(), load_task_middleware));

    let inner = Router::new()
        .route("/", get(get_tasks).post(create_task))
        .route("/backlog", get(get_backlog))
        .route("/stream/ws", get(stream_tasks_ws))
        .route("/create-and-start", post(create_task_and_start))
        .nest("/{task_id}", task_id_router);

    // mount under /projects/:project_id/tasks
    Router::new().nest("/tasks", inner)
}

#[cfg(test)]
mod tests {
    use db::models::{
        project::{CreateProject, Project},
        sprint::{CreateSprint, Sprint},
        task::{CreateTask, Task, TaskStatus, TaskType},
    };
    use sqlx::SqlitePool;
    use uuid::Uuid;

    use super::validate_and_normalize_agile_fields;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        pool
    }

    async fn create_project(pool: &SqlitePool) -> Project {
        let project_id = Uuid::new_v4();
        Project::create(
            pool,
            &CreateProject {
                name: "Test".to_string(),
                repositories: vec![],
            },
            project_id,
        )
        .await
        .unwrap()
    }

    async fn create_sprint(pool: &SqlitePool, project_id: Uuid) -> Sprint {
        Sprint::create(
            pool,
            &CreateSprint {
                project_id,
                name: "Sprint 1".to_string(),
                goal: None,
                start_date: None,
                end_date: None,
                status: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap()
    }

    async fn create_task(pool: &SqlitePool, project_id: Uuid, task_type: TaskType) -> Task {
        Task::create(
            pool,
            &CreateTask {
                project_id,
                title: format!("{task_type}"),
                description: None,
                status: Some(TaskStatus::Todo),
                sprint_id: None,
                task_type: Some(task_type),
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
    async fn validates_epic_cannot_be_in_sprint() {
        let pool = setup_pool().await;
        let project = create_project(&pool).await;
        let sprint = create_sprint(&pool, project.id).await;

        let err = validate_and_normalize_agile_fields(
            &pool,
            project.id,
            TaskType::Epic,
            Some(sprint.id),
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Epic tasks cannot be assigned to a sprint"));
    }

    #[tokio::test]
    async fn validates_feature_requires_epic_parent_and_derives_epic_id() {
        let pool = setup_pool().await;
        let project = create_project(&pool).await;
        let epic = create_task(&pool, project.id, TaskType::Epic).await;

        let (sprint_id, epic_id, parent_task_id, story_points) = validate_and_normalize_agile_fields(
            &pool,
            project.id,
            TaskType::Feature,
            None,
            None,
            Some(epic.id),
            None,
        )
        .await
        .unwrap();

        assert_eq!(sprint_id, None);
        assert_eq!(epic_id, Some(epic.id));
        assert_eq!(parent_task_id, Some(epic.id));
        assert_eq!(story_points, None);
    }

    #[tokio::test]
    async fn validates_story_requires_feature_parent_and_derives_epic_id() {
        let pool = setup_pool().await;
        let project = create_project(&pool).await;
        let epic = create_task(&pool, project.id, TaskType::Epic).await;

        let feature = Task::create(
            &pool,
            &CreateTask {
                project_id: project.id,
                title: "Feature".to_string(),
                description: None,
                status: Some(TaskStatus::Todo),
                sprint_id: None,
                task_type: Some(TaskType::Feature),
                epic_id: Some(epic.id),
                parent_task_id: Some(epic.id),
                story_points: None,
                parent_workspace_id: None,
                image_ids: None,
                shared_task_id: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap();

        let (_, epic_id, parent_task_id, story_points) = validate_and_normalize_agile_fields(
            &pool,
            project.id,
            TaskType::Story,
            None,
            None,
            Some(feature.id),
            Some(3),
        )
        .await
        .unwrap();

        assert_eq!(epic_id, Some(epic.id));
        assert_eq!(parent_task_id, Some(feature.id));
        assert_eq!(story_points, Some(3));
    }
}
