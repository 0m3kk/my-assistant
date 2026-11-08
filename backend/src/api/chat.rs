use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::AuthUser;
use crate::error::AppResult;
use crate::service::chat::{self, ChatRequest};
use crate::state::AppState;

pub async fn chat(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<ChatRequest>,
) -> AppResult<
    axum::response::Sse<
        impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
> {
    chat::run_chat(state, user, payload).await
}

#[derive(Debug, Deserialize)]
pub struct CancelRequest {
    pub conversation_id: Uuid,
}

pub async fn cancel(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<CancelRequest>,
) -> AppResult<axum::Json<serde_json::Value>> {
    chat::cancel_chat(state, user, payload.conversation_id).await?;
    Ok(axum::Json(serde_json::json!({"cancelled": true})))
}

#[derive(Debug, Deserialize)]
pub struct ResumeQuery {
    pub conversation_id: Uuid,
}

/// `GET /api/chat/resume?conversation_id=…` — replay + follow a run's stream.
/// Responds 204 when there is no replayable stream for the conversation.
pub async fn resume(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<ResumeQuery>,
) -> AppResult<axum::response::Response> {
    crate::repo::conversations::owned_by_id(&state.pool, q.conversation_id, user.id).await?;
    let Some(run_stream) = state
        .run_streams
        .get(&q.conversation_id)
        .map(|entry| Arc::clone(entry.value()))
    else {
        return Ok(axum::http::StatusCode::NO_CONTENT.into_response());
    };

    let sub = run_stream.subscribe();
    let stream = futures::stream::unfold(sub, |mut sub| async move {
        sub.next().await.map(|item| {
            let event = axum::response::sse::Event::default().data(item.chunk.to_wire());
            (Ok::<_, std::convert::Infallible>(event), sub)
        })
    });
    Ok(axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response())
}
