use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::service::proposals;
use crate::state::AppState;

pub async fn list_tool_versions(
    Path(tool_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<Value>> {
    let rows = repo::versions::list_tool_versions(&state.pool, tool_id).await?;
    Ok(Json(json!({"versions": rows})))
}

pub async fn list_agent_versions(
    Path(agent_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<Value>> {
    let rows = repo::versions::list_agent_versions(&state.pool, agent_id).await?;
    Ok(Json(json!({"versions": rows})))
}

/// Create a pending proposal that restores a previous tool version.
pub async fn restore_tool(
    Path((tool_id, revision)): Path<(Uuid, i32)>,
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
) -> AppResult<Json<Value>> {
    let version = repo::versions::tool_version(&state.pool, tool_id, revision)
        .await?
        .ok_or_else(|| AppError::not_found("tool version not found"))?;
    let s = &version.snapshot;
    let payload = json!({
        "name": s["name"],
        "display_name": s["display_name"],
        "description": s["description"],
        "kind": s["kind"],
        "config": s["config"],
        "parameters_schema": s["parameters_schema"],
        "requires_approval": s["requires_approval"],
    });
    proposals::validate_tool(&payload)?;
    let before = repo::tools::snapshot(&state.pool, tool_id).await?;
    let proposal = repo::proposals::create(
        &state.pool,
        "tool",
        "update",
        Some(tool_id),
        &format!("Restore tool revision {revision}"),
        "Restore a previous version for admin review.",
        payload,
        Some(before),
        &[],
        Some(admin.id),
        None,
        None,
    )
    .await?;
    Ok(Json(json!({
        "proposal_id": proposal.id,
        "status": proposal.status,
        "title": proposal.title,
    })))
}

/// Create a pending proposal that restores a previous agent version.
pub async fn restore_agent(
    Path((agent_id, revision)): Path<(Uuid, i32)>,
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
) -> AppResult<Json<Value>> {
    let version = repo::versions::agent_version(&state.pool, agent_id, revision)
        .await?
        .ok_or_else(|| AppError::not_found("agent version not found"))?;
    let s = &version.snapshot;
    let agent = &s["agent"];
    let payload = json!({
        "name": agent["name"],
        "slug": agent["slug"],
        "description": agent["description"],
        "system_prompt": agent["system_prompt"],
        "default_model_id": agent["default_model_id"],
        "temperature": agent["temperature"],
        "max_tokens": agent["max_tokens"],
        "max_tool_calls": agent["max_tool_calls"],
        "tools": s["tools"],
    });
    proposals::validate_agent(&state.pool, &payload).await?;
    let before = repo::agents::snapshot(&state.pool, agent_id).await?;
    let proposal = repo::proposals::create(
        &state.pool,
        "agent",
        "update",
        Some(agent_id),
        &format!("Restore agent revision {revision}"),
        "Restore a previous version for admin review.",
        payload,
        Some(before),
        &[],
        Some(admin.id),
        None,
        None,
    )
    .await?;
    Ok(Json(json!({
        "proposal_id": proposal.id,
        "status": proposal.status,
        "title": proposal.title,
    })))
}
