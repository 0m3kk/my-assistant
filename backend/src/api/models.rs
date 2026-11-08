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
pub struct ModelOut {
    pub id: Uuid,
    pub provider_id: Uuid,
    /// Provider display name — lets the selector group models by provider.
    pub provider_name: String,
    pub display_name: String,
    pub api_name: String,
    /// Model context window (`null` when the provider published none).
    pub context_window: Option<i32>,
    pub is_default: bool,
    pub capabilities: serde_json::Value,
    pub reasoning_effort: Option<String>,
}

/// Enabled models available to any signed-in user (Model Selector).
pub async fn list(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
) -> AppResult<Json<Vec<ModelOut>>> {
    let models = repo::providers::list_enabled_models_with_provider(&state.pool).await?;
    Ok(Json(
        models
            .into_iter()
            .map(|(m, provider_name)| ModelOut {
                id: m.id,
                provider_id: m.provider_id,
                provider_name,
                display_name: if m.display_name.is_empty() {
                    m.api_name.clone()
                } else {
                    m.display_name
                },
                api_name: m.api_name,
                context_window: m.context_window,
                is_default: m.is_default,
                capabilities: m.capabilities,
                reasoning_effort: m.reasoning_effort,
            })
            .collect(),
    ))
}
