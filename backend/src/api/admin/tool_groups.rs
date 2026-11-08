use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::domain::ToolGroup;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct GroupOut {
    pub id: Uuid,
    pub name: String,
    pub position: i32,
}

impl From<&ToolGroup> for GroupOut {
    fn from(g: &ToolGroup) -> Self {
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
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let groups = repo::tool_groups::list(&state.pool).await?;
    let out: Vec<GroupOut> = groups.iter().map(GroupOut::from).collect();
    Ok(Json(serde_json::json!({ "groups": out })))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<NameRequest>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    let name = validate_name(&payload.name)?;
    let group = repo::tool_groups::create(&state.pool, name).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(GroupOut::from(&group))?),
    ))
}

pub async fn update(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<NameRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let name = validate_name(&payload.name)?;
    let group = repo::tool_groups::rename(&state.pool, id, name).await?;
    Ok(Json(serde_json::to_value(GroupOut::from(&group))?))
}

pub async fn remove(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<StatusCode> {
    repo::tool_groups::delete(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
