use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateAgent {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub system_prompt: String,
    pub default_model_id: Option<Uuid>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub max_tool_calls: Option<i32>,
}

fn validate_slug(slug: &str) -> AppResult<()> {
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(AppError::validation(
            "slug must be lowercase alphanumeric with - or _",
        ));
    }
    Ok(())
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let agents = repo::agents::list(&state.pool).await?;
    Ok(Json(serde_json::json!({"agents": agents})))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Json(payload): Json<CreateAgent>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    if payload.name.trim().is_empty() {
        return Err(AppError::validation("name is required"));
    }
    validate_slug(&payload.slug)?;
    let agent = repo::agents::create(
        &state.pool,
        payload.name.trim(),
        &payload.slug,
        &payload.description,
        &payload.system_prompt,
        payload.default_model_id,
        payload.temperature,
        payload.max_tokens,
        payload.max_tool_calls.unwrap_or(200),
        Some(admin.id),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(&agent)?)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateAgent {
    pub name: Option<String>,
    pub description: Option<String>,
    pub system_prompt: Option<String>,
    pub default_model_id: Option<Option<Uuid>>,
    pub temperature: Option<Option<f32>>,
    pub max_tokens: Option<Option<i32>>,
    pub max_tool_calls: Option<i32>,
    pub status: Option<String>,
    pub group_id: Option<Option<Uuid>>,
}

pub async fn update(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<UpdateAgent>,
) -> AppResult<Json<serde_json::Value>> {
    if let Some(Some(group_id)) = payload.group_id {
        repo::agent_groups::by_id(&state.pool, group_id)
            .await?
            .ok_or_else(|| AppError::bad_request("agent folder not found"))?;
    }
    if let Some(name) = payload.name.as_deref() {
        if name.trim().is_empty() {
            return Err(AppError::validation("name is required"));
        }
    }
    let agent = repo::agents::update(
        &state.pool,
        id,
        payload.name.as_deref().map(str::trim),
        payload.description.as_deref(),
        payload.system_prompt.as_deref(),
        payload.default_model_id,
        payload.temperature,
        payload.max_tokens,
        payload.max_tool_calls,
        payload.status.as_deref(),
        payload.group_id,
    )
    .await?;
    Ok(Json(serde_json::to_value(&agent)?))
}

pub async fn delete(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<StatusCode> {
    repo::agents::delete(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Agent-tool assignments ----------

#[derive(Debug, Deserialize)]
pub struct AssignTool {
    pub tool_id: Uuid,
    pub requires_approval: Option<bool>,
    /// Per-agent override config, e.g. {"allowed_paths": ["/home/me/project"]}.
    pub config: Option<serde_json::Value>,
}

pub async fn list_tools(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let rows = repo::agents::tools_for_agent(&state.pool, id).await?;
    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(a, t)| {
            serde_json::json!({
                "tool_id": t.id,
                "name": t.name,
                "display_name": t.display_name,
                "kind": t.kind,
                "requires_approval": a.requires_approval.unwrap_or(t.requires_approval),
                "approval_override": a.requires_approval.is_some(),
                "config": a.config,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({"tools": out})))
}

pub async fn assign_tool(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<AssignTool>,
) -> AppResult<StatusCode> {
    repo::agents::by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("agent not found"))?;
    repo::tools::by_id(&state.pool, payload.tool_id)
        .await?
        .ok_or_else(|| AppError::not_found("tool not found"))?;
    repo::agents::assign_tool(
        &state.pool,
        id,
        payload.tool_id,
        payload.requires_approval,
        payload.config,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unassign_tool(
    Path((id, tool_id)): Path<(Uuid, Uuid)>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<StatusCode> {
    repo::agents::unassign_tool(&state.pool, id, tool_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
