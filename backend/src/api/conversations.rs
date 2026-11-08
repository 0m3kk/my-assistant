use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::AuthUser;
use crate::domain::Conversation;
use crate::error::AppResult;
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ConversationSummary {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub title: String,
    pub status: String,
    pub group_id: Option<Uuid>,
    pub last_message_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<&Conversation> for ConversationSummary {
    fn from(c: &Conversation) -> Self {
        Self {
            id: c.id,
            agent_id: c.agent_id,
            title: c.title.clone(),
            status: c.status.clone(),
            group_id: c.group_id,
            last_message_at: c.last_message_at,
            created_at: c.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MessageOut {
    pub id: Uuid,
    pub role: String,
    pub parts: serde_json::Value,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<Json<Vec<ConversationSummary>>> {
    let rows = repo::conversations::list_for_user(&state.pool, user.id).await?;
    Ok(Json(rows.iter().map(ConversationSummary::from).collect()))
}

pub async fn messages(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<Json<Vec<MessageOut>>> {
    repo::conversations::owned_by_id(&state.pool, id, user.id).await?;
    let rows = repo::conversations::messages(&state.pool, id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|m| MessageOut {
                id: m.id,
                role: m.role,
                parts: m.parts,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    #[serde(deserialize_with = "non_empty")]
    pub title: String,
}

fn non_empty<'de, D>(de: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(de)?;
    if s.trim().is_empty() {
        return Err(serde::de::Error::custom("title must not be empty"));
    }
    Ok(s)
}

pub async fn rename(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<RenameRequest>,
) -> AppResult<Json<ConversationSummary>> {
    repo::conversations::owned_by_id(&state.pool, id, user.id).await?;
    let c = repo::conversations::rename(&state.pool, id, payload.title.trim()).await?;
    Ok(Json(ConversationSummary::from(&c)))
}

pub async fn generate_title(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<Json<ConversationSummary>> {
    repo::conversations::owned_by_id(&state.pool, id, user.id).await?;
    let c = crate::service::title::generate_now(&state, id, user.id).await?;
    Ok(Json(ConversationSummary::from(&c)))
}

#[derive(Debug, Deserialize)]
pub struct SetGroupRequest {
    /// `null` (or an absent field) moves the conversation out of any folder.
    pub group_id: Option<Uuid>,
}

pub async fn set_group(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<SetGroupRequest>,
) -> AppResult<Json<ConversationSummary>> {
    repo::conversations::owned_by_id(&state.pool, id, user.id).await?;
    if let Some(group_id) = payload.group_id {
        repo::conversation_groups::owned_by_id(&state.pool, group_id, user.id).await?;
    }
    let c = repo::conversations::set_group(&state.pool, id, user.id, payload.group_id).await?;
    Ok(Json(ConversationSummary::from(&c)))
}

pub async fn delete(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<StatusCode> {
    // Also cancel an active run if any.
    if let Some(entry) = state.runs.get(&id) {
        entry.value().cancel();
    }
    repo::conversations::delete(&state.pool, id, user.id).await?;
    state.run_streams.remove(&id);
    // Best-effort cleanup of the conversation sandbox. The DB row is already
    // gone, so a failure here only leaves an orphaned directory behind.
    let sandbox = crate::agent::sandbox_root_for(&state.cfg, id);
    match tokio::fs::remove_dir_all(&sandbox).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(conversation_id = %id, error = %e, "failed to remove conversation workspace")
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
