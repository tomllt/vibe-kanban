use axum::{
    Router,
    extract::{
        Query, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use deployment::Deployment;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use serde::Deserialize;
use uuid::Uuid;

use crate::DeploymentImpl;

#[derive(Debug, Deserialize)]
pub struct MergesQuery {
    pub workspace_id: Uuid,
    pub repo_id: Uuid,
}

pub async fn stream_merges_ws(
    ws: WebSocketUpgrade,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<MergesQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_merges_ws(socket, deployment, query.workspace_id, query.repo_id).await
        {
            tracing::warn!("merges WS closed: {}", e);
        }
    })
}

async fn handle_merges_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    workspace_id: Uuid,
    repo_id: Uuid,
) -> anyhow::Result<()> {
    let mut stream = deployment
        .events()
        .stream_merges_raw(workspace_id, repo_id)
        .await?
        .map_ok(|msg| msg.to_ws_message_unchecked());

    let (mut sender, mut receiver) = socket.split();
    tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });

    while let Some(item) = stream.next().await {
        match item {
            Ok(msg) => {
                if sender.send(msg).await.is_err() {
                    break;
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

pub fn router(_: &DeploymentImpl) -> Router<DeploymentImpl> {
    Router::new().route("/merges/stream/ws", get(stream_merges_ws))
}

