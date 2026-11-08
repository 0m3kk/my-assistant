use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::error::AppResult;
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
    pub tool_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub limit: Option<i64>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = repo::feedback::list_admin(
        &state.pool,
        q.status.as_deref(),
        q.tool_id,
        q.agent_id,
        limit,
    )
    .await?;
    Ok(Json(serde_json::json!({"feedback": rows})))
}

#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    /// open | addressed | dismissed
    pub status: String,
}

pub async fn update(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<UpdateBody>,
) -> AppResult<Json<serde_json::Value>> {
    let row = repo::feedback::set_status(&state.pool, id, &body.status).await?;
    Ok(Json(serde_json::to_value(&row)?))
}
