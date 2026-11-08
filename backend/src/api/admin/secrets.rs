use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::api::admin::AdminUser;
use crate::error::{AppError, AppResult};
use crate::secrets::SecretService;
use crate::state::AppState;

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let secrets = SecretService::list(&state.pool).await?;
    Ok(Json(serde_json::json!({"secrets": secrets})))
}

#[derive(Debug, Deserialize)]
pub struct CreateSecret {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub value: String,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Json(payload): Json<CreateSecret>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    let secret = SecretService::create(
        &state.pool,
        &state.cfg.app_secret,
        &state.secrets,
        &payload.name,
        &payload.description,
        &payload.value,
        admin.0.id,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(&secret)?)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateSecret {
    pub description: Option<String>,
    /// Empty/absent keeps the current value (rotate by sending a new value).
    pub value: Option<String>,
}

pub async fn update(
    Path(id): Path<uuid::Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<UpdateSecret>,
) -> AppResult<Json<serde_json::Value>> {
    let secret = SecretService::update(
        &state.pool,
        &state.cfg.app_secret,
        &state.secrets,
        id,
        payload.description.as_deref(),
        payload.value.as_deref(),
    )
    .await?;
    Ok(Json(serde_json::to_value(&secret)?))
}

pub async fn delete(
    Path(id): Path<uuid::Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<StatusCode> {
    SecretService::delete(&state.pool, &state.secrets, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[allow(dead_code)]
fn _unused(e: AppError) -> AppError {
    e
}
