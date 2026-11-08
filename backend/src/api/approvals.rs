use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::AuthUser;
use crate::domain::{Approval, Conversation};
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::service::proposals::{self, ProposeRequest};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct DecisionRequest {
    pub decision: String, // "approved" | "denied"
    /// "once" | "propose_update" (out-of-root approvals)
    pub decision_mode: Option<String>,
}

pub async fn decide(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<DecisionRequest>,
) -> AppResult<Json<Approval>> {
    let approval = repo::approvals::by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("approval not found"))?;
    // Ownership through the conversation.
    let conversation =
        repo::conversations::owned_by_id(&state.pool, approval.conversation_id, user.id).await?;
    if approval.status != "pending" {
        return Err(AppError::conflict("approval already decided"));
    }
    let status = match payload.decision.as_str() {
        "approved" => "approved",
        "denied" => "denied",
        other => return Err(AppError::bad_request(format!("invalid decision: {other}"))),
    };
    let mode = payload.decision_mode.as_deref();
    if let Some(m) = mode {
        if !matches!(m, "once" | "propose_update") {
            return Err(AppError::bad_request("invalid decision_mode"));
        }
    }
    let updated = repo::approvals::decide(&state.pool, id, status, mode).await?;

    // "Allow + propose update": keep the one-off approval, and queue a pending
    // tool proposal so an admin can review broadening the scope.
    if status == "approved" && mode == Some("propose_update") {
        propose_scope_update(&state, user.id, &conversation, &approval).await;
    }

    crate::repo::audit::insert(
        &state.pool,
        Some(user.id),
        "approval.decide",
        &format!("tool={} call={}", approval.tool_name, approval.tool_call_id),
        true,
        serde_json::json!({"decision": status, "decision_mode": mode}),
    )
    .await
    .ok();
    Ok(Json(updated))
}

async fn propose_scope_update(
    state: &AppState,
    user_id: Uuid,
    conversation: &Conversation,
    approval: &Approval,
) {
    let Some(resource) = approval.requested_resource.as_deref() else {
        return;
    };
    let Ok(Some(tool)) = repo::tools::by_name(&state.pool, &approval.tool_name).await else {
        return;
    };
    if tool.is_builtin || tool.is_meta {
        return;
    }
    let Ok(mut payload) = serde_json::to_value(&tool) else {
        return;
    };
    if let Some(config) = payload.get_mut("config").and_then(Value::as_object_mut) {
        let globs = config.entry("allow_globs").or_insert_with(|| json!([]));
        if let Some(arr) = globs.as_array_mut() {
            if !arr.iter().any(|v| v.as_str() == Some(resource)) {
                arr.push(json!(resource));
            }
        }
    }
    let req = ProposeRequest {
        resource: "tool".into(),
        action: "update".into(),
        target_id: Some(tool.id),
        title: format!("Allow {}", resource),
        rationale: format!(
            "The user allowed out-of-root access to {resource}. Broaden the tool scope for review."
        ),
        payload,
        feedback_ids: Vec::new(),
    };
    if let Err(e) = proposals::propose(
        &state.pool,
        user_id,
        conversation.agent_id,
        Some(conversation.id),
        req,
    )
    .await
    {
        tracing::warn!(error = %e, "failed to create out-of-root scope proposal");
    }
}

pub async fn list_for_conversation(
    Path(conversation_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<Json<Vec<Approval>>> {
    repo::conversations::owned_by_id(&state.pool, conversation_id, user.id).await?;
    let rows = repo::approvals::list_for_conversation(&state.pool, conversation_id).await?;
    Ok(Json(rows))
}
