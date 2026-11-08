//! Workspace exploration routes: browse + download the per-conversation
//! sandbox files the agent produced during its work.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::stream;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::AuthUser;
use crate::config::AppMode;
use crate::domain::WorkspaceEntry;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;
use crate::tools::fs::PathGuard;

/// Upper bound for a single workspace download.
const MAX_WORKSPACE_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// `GET /api/conversations/{id}/workspace` — recursive sandbox tree.
pub async fn tree(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
) -> AppResult<Json<Vec<WorkspaceEntry>>> {
    repo::conversations::owned_by_id(&state.pool, id, user.id).await?;
    let sandbox_root = crate::agent::sandbox_root_for(&state.cfg, id);
    Ok(Json(
        crate::service::workspace::list_tree(&sandbox_root).await,
    ))
}

#[derive(Debug, Deserialize)]
pub struct ContentQuery {
    /// Path relative to the conversation sandbox (or absolute inside it).
    pub path: String,
}

/// `GET /api/conversations/{id}/workspace/content?path=…` — stream one file
/// from the sandbox (read-only, PathGuard-confined, download-safe headers).
pub async fn content(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
    Query(q): Query<ContentQuery>,
) -> AppResult<Response> {
    repo::conversations::owned_by_id(&state.pool, id, user.id).await?;
    let sandbox_root = crate::agent::sandbox_root_for(&state.cfg, id);
    // The conversation workspace is always sandbox-confined, even in local
    // mode (local-mode whitelists are a tool-level concern, not exploration).
    let guard = PathGuard::new(AppMode::Server, sandbox_root.clone(), vec![]);
    let resolved = guard
        .resolve(&q.path, false)
        .map_err(|e| AppError::forbidden(e.to_string()))?;
    if !resolved.is_file() {
        return Err(AppError::not_found("file not found in workspace"));
    }
    let meta = tokio::fs::metadata(&resolved)
        .await
        .map_err(|e| AppError::not_found(format!("cannot stat file: {e}")))?;
    if meta.len() > MAX_WORKSPACE_FILE_BYTES {
        return Err(AppError::payload_too_large(
            "workspace file exceeds the download limit (64MB)",
        ));
    }

    let filename = resolved
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let content_type = crate::storage::guess_content_type(&filename);
    let data = tokio::fs::read(&resolved)
        .await
        .map_err(|e| AppError::internal(format!("cannot read file: {e}")))?;

    let body = axum::body::Body::from_stream(stream::once(async move {
        Ok::<Bytes, std::convert::Infallible>(Bytes::from(data))
    }));
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename.replace('"', "_")),
            ),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff".to_string(),
            ),
        ],
        body,
    )
        .into_response())
}
