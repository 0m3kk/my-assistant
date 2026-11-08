use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use uuid::Uuid;

use crate::api::AuthUser;
use crate::error::AppResult;
use crate::repo;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct AgentOption {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: String,
    /// Model pinned on the agent, if any — the selector can preselect it.
    pub default_model_id: Option<Uuid>,
    pub is_builtin: bool,
}

/// The agent the chat surface runs on. Phase 1 exposes exactly one: the
/// default `general` assistant, which decides for itself how to carry out the
/// task. Picking other agents is intentionally not offered yet.
pub async fn list(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
) -> AppResult<Json<Vec<AgentOption>>> {
    let agent = repo::agents::by_slug(&state.pool, "general")
        .await?
        .filter(|a| a.status == "enabled" && a.kind == "chat");
    Ok(Json(
        agent
            .into_iter()
            .map(|a| AgentOption {
                id: a.id,
                name: a.name,
                slug: a.slug,
                description: a.description,
                default_model_id: a.default_model_id,
                is_builtin: a.is_builtin,
            })
            .collect(),
    ))
}
