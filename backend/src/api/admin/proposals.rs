use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::error::AppResult;
use crate::repo;
use crate::service::proposals;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
    pub resource: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let rows =
        repo::proposals::list(&state.pool, q.status.as_deref(), q.resource.as_deref()).await?;
    Ok(Json(serde_json::json!({"proposals": rows})))
}

pub async fn get(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let row = repo::proposals::by_id(&state.pool, id)
        .await?
        .ok_or_else(|| crate::error::AppError::not_found("proposal not found"))?;
    Ok(Json(serde_json::to_value(&row)?))
}

#[derive(Debug, Deserialize)]
pub struct ApplyBody {
    /// Optional admin edits applied on top of the proposal payload.
    pub payload: Option<Value>,
    #[serde(default)]
    pub force: bool,
}

pub async fn apply(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Json(body): Json<ApplyBody>,
) -> AppResult<Json<serde_json::Value>> {
    let row = proposals::apply(&state.pool, id, admin.id, body.payload, body.force).await?;
    Ok(Json(serde_json::to_value(&row)?))
}

pub async fn reject(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let row = proposals::reject(&state.pool, id, admin.id).await?;
    Ok(Json(serde_json::to_value(&row)?))
}
