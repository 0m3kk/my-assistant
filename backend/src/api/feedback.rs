use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::AuthUser;
use crate::error::AppResult;
use crate::service::feedback::{self, SubmitFeedback};
use crate::state::AppState;

pub async fn submit(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<SubmitFeedback>,
) -> AppResult<Json<serde_json::Value>> {
    let row = feedback::submit(&state.pool, user.id, payload).await?;
    Ok(Json(serde_json::to_value(&row)?))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub conversation_id: Uuid,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let rows = feedback::list_for_conversation(&state.pool, user.id, q.conversation_id).await?;
    Ok(Json(serde_json::json!({"feedback": rows})))
}

pub async fn delete(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<StatusCode> {
    feedback::delete(&state.pool, user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
