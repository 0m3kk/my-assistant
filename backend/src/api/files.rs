//! Attachment routes: upload (multipart), download, delete-pending.

use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::api::AuthUser;
use crate::domain::FileDto;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::service::files as files_service;
use crate::state::AppState;

/// `POST /api/files` — upload one attachment (multipart field `file`).
pub async fn upload(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    multipart: Multipart,
) -> AppResult<Json<FileDto>> {
    let upload = files_service::extract_upload(multipart, state.cfg.max_upload_bytes).await?;
    let row = files_service::store_attachment(&state, user.id, upload).await?;
    Ok(Json(row.to_dto()))
}

fn content_disposition(filename: &str) -> String {
    // ASCII fallback + RFC 5987 encoding for non-ASCII names.
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        urlencoding_encode(filename)
    )
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Serve a file's bytes with download-safe headers. Shared by attachment and
/// (via the workspace routes) sandbox content.
pub fn file_response(
    content_type: &str,
    filename: &str,
    stream: crate::storage::StorageStream,
) -> Response {
    let body = Body::from_stream(stream);
    (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_DISPOSITION, content_disposition(filename)),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff".to_string(),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
        ],
        body,
    )
        .into_response()
}

/// `GET /api/files/{id}/content` — stream the file bytes (owner or admin).
pub async fn download(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
) -> AppResult<Response> {
    let row = sqlx::query_as::<_, crate::domain::FileRow>("SELECT * FROM files WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("file not found"))?;
    if row.user_id != user.id && user.role != "admin" {
        return Err(AppError::not_found("file not found"));
    }
    let stream = state
        .storage
        .get(&row.storage_key)
        .await
        .map_err(|e| match e {
            crate::storage::StorageError::NotFound(_) => {
                AppError::not_found("file content missing")
            }
            other => AppError::internal(format!("storage read failed: {other}")),
        })?;
    Ok(file_response(&row.content_type, &row.filename, stream))
}

/// `DELETE /api/files/{id}` — remove a pending attachment (not yet sent).
pub async fn remove(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    user: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    let row = repo::files::owned_by_id(&state.pool, id, user.id).await?;
    if row.message_id.is_some() || row.conversation_id.is_some() {
        return Err(AppError::conflict(
            "file is already attached to a message and cannot be removed",
        ));
    }
    state
        .storage
        .delete(&row.storage_key)
        .await
        .map_err(|e| AppError::internal(format!("storage delete failed: {e}")))?;
    repo::files::delete(&state.pool, id).await?;
    repo::audit::insert(
        &state.pool,
        Some(user.id),
        "file.delete",
        &row.filename,
        true,
        serde_json::json!({ "size": row.size_bytes }),
    )
    .await
    .ok();
    Ok(Json(serde_json::json!({ "deleted": true })))
}
