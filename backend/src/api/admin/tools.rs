use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateTool {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    /// "http" | "file" | "wasm"
    pub kind: String,
    pub config: serde_json::Value,
    pub parameters_schema: serde_json::Value,
    #[serde(default)]
    pub requires_approval: bool,
}

fn validate_name(name: &str) -> AppResult<()> {
    if name.is_empty()
        || !name.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.'
        })
    {
        return Err(AppError::validation(
            "tool name must be lowercase [a-z0-9_.-]",
        ));
    }
    Ok(())
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let tools = repo::tools::list_visible(&state.pool).await?;
    Ok(Json(serde_json::json!({"tools": tools})))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Json(payload): Json<CreateTool>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    validate_name(&payload.name)?;
    let body = serde_json::json!({
        "name": payload.name,
        "display_name": payload.display_name.clone().unwrap_or_else(|| payload.name.clone()),
        "description": payload.description.clone().unwrap_or_default(),
        "kind": payload.kind,
        "config": payload.config,
        "parameters_schema": payload.parameters_schema,
        "requires_approval": payload.requires_approval,
    });
    let v = crate::service::proposals::validate_tool(&body)?;

    let tool = repo::tools::create(
        &state.pool,
        &v.name,
        &v.display_name,
        &v.description,
        &v.kind,
        "",
        v.config.clone(),
        v.parameters_schema.clone(),
        v.requires_approval,
        false,
        false,
        Some(admin.id),
    )
    .await?;

    if v.kind == "wasm" {
        let source = v
            .config
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let language = v
            .config
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("rust");
        repo::tool_builds::create_pending(
            &state.pool,
            tool.id,
            tool.revision,
            language,
            source,
            "wasm",
            Some(admin.id),
        )
        .await?;
        sqlx::query("UPDATE tools SET status = 'disabled' WHERE id = $1")
            .bind(tool.id)
            .execute(&state.pool)
            .await?;
    }

    Ok((StatusCode::CREATED, Json(serde_json::to_value(&tool)?)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateTool {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub config: Option<serde_json::Value>,
    pub parameters_schema: Option<serde_json::Value>,
    pub requires_approval: Option<bool>,
    pub status: Option<String>,
    pub group_id: Option<Option<Uuid>>,
}

pub async fn update(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<UpdateTool>,
) -> AppResult<Json<serde_json::Value>> {
    if let Some(Some(group_id)) = payload.group_id {
        repo::tool_groups::by_id(&state.pool, group_id)
            .await?
            .ok_or_else(|| AppError::bad_request("tool folder not found"))?;
    }
    let tool = repo::tools::update(
        &state.pool,
        id,
        payload.display_name.as_deref(),
        payload.description.as_deref(),
        payload.config,
        payload.parameters_schema,
        payload.requires_approval,
        payload.status.as_deref(),
        payload.group_id,
    )
    .await?;
    Ok(Json(serde_json::to_value(&tool)?))
}

pub async fn delete(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<StatusCode> {
    repo::tools::delete(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
