use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::AuthUser;
use crate::domain::ConversationGroup;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct GroupOut {
    pub id: Uuid,
    pub name: String,
    pub position: i32,
}

impl From<&ConversationGroup> for GroupOut {
    fn from(g: &ConversationGroup) -> Self {
        Self {
            id: g.id,
            name: g.name.clone(),
            position: g.position,
        }
    }
}

const MAX_NAME_CHARS: usize = 60;

fn validate_name(name: &str) -> AppResult<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("folder name must not be empty"));
    }
    if trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::validation("folder name is too long"));
    }
    Ok(trimmed)
}

#[derive(Debug, Deserialize)]
pub struct NameRequest {
    pub name: String,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<Json<Vec<GroupOut>>> {
    let rows = repo::conversation_groups::list_for_user(&state.pool, user.id).await?;
    Ok(Json(rows.iter().map(GroupOut::from).collect()))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<NameRequest>,
) -> AppResult<(StatusCode, Json<GroupOut>)> {
    let name = validate_name(&payload.name)?;
    let group = repo::conversation_groups::create(&state.pool, user.id, name).await?;
    Ok((StatusCode::CREATED, Json(GroupOut::from(&group))))
}

pub async fn rename(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<NameRequest>,
) -> AppResult<Json<GroupOut>> {
    let name = validate_name(&payload.name)?;
    let group = repo::conversation_groups::rename(&state.pool, id, user.id, name).await?;
    Ok(Json(GroupOut::from(&group)))
}

pub async fn remove(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<StatusCode> {
    repo::conversation_groups::delete(&state.pool, id, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
