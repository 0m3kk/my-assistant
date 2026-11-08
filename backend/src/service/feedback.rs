use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::FeedbackRow;
use crate::error::{AppError, AppResult};
use crate::repo;

#[derive(Debug, Deserialize)]
pub struct SubmitFeedback {
    /// "tool_run" | "agent_run"
    pub scope: String,
    pub conversation_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub tool_call_id: Option<String>,
    /// -1 (down) or 1 (up)
    pub rating: i16,
    #[serde(default)]
    pub comment: String,
}

pub async fn submit(pool: &PgPool, user_id: Uuid, req: SubmitFeedback) -> AppResult<FeedbackRow> {
    if req.rating != -1 && req.rating != 1 {
        return Err(AppError::validation("rating must be -1 or 1"));
    }
    let conversation_id = req
        .conversation_id
        .ok_or_else(|| AppError::validation("conversation_id is required"))?;
    // Ownership check: users may only rate their own conversations.
    repo::conversations::owned_by_id(pool, conversation_id, user_id).await?;

    let comment: String = req.comment.trim().chars().take(4000).collect();

    match req.scope.as_str() {
        "tool_run" => {
            let tool_call_id = req
                .tool_call_id
                .ok_or_else(|| AppError::validation("tool_call_id is required for tool_run"))?;
            let execution = repo::tool_executions::by_conversation_and_tool_call(
                pool,
                conversation_id,
                &tool_call_id,
            )
            .await?
            .ok_or_else(|| AppError::not_found("tool execution not found"))?;
            let subject_key = format!("tool:{conversation_id}:{tool_call_id}");
            repo::feedback::upsert(
                pool,
                user_id,
                "tool_run",
                Some(conversation_id),
                execution.run_id,
                Some(execution.id),
                execution.tool_id,
                None,
                req.rating,
                &comment,
                &subject_key,
            )
            .await
        }
        "agent_run" => {
            let run_id = req
                .run_id
                .ok_or_else(|| AppError::validation("run_id is required for agent_run"))?;
            let run = repo::runs::by_id(pool, run_id)
                .await?
                .ok_or_else(|| AppError::not_found("run not found"))?;
            if run.conversation_id != conversation_id {
                return Err(AppError::not_found("run not found"));
            }
            let subject_key = format!("agent:{run_id}");
            repo::feedback::upsert(
                pool,
                user_id,
                "agent_run",
                Some(conversation_id),
                Some(run_id),
                None,
                None,
                Some(run.agent_id),
                req.rating,
                &comment,
                &subject_key,
            )
            .await
        }
        other => Err(AppError::validation(format!(
            "unknown feedback scope: {other}"
        ))),
    }
}

pub async fn list_for_conversation(
    pool: &PgPool,
    user_id: Uuid,
    conversation_id: Uuid,
) -> AppResult<Vec<FeedbackRow>> {
    repo::conversations::owned_by_id(pool, conversation_id, user_id).await?;
    repo::feedback::list_for_conversation(pool, user_id, conversation_id).await
}

pub async fn delete(pool: &PgPool, user_id: Uuid, id: Uuid) -> AppResult<()> {
    let row = repo::feedback::by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("feedback not found"))?;
    if row.user_id != user_id {
        return Err(AppError::not_found("feedback not found"));
    }
    repo::feedback::delete(pool, id, user_id).await
}
