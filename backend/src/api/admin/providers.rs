use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::domain::PROVIDER_PRESETS;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

const VALID_PRESETS: &[&str] = &[
    "glm",
    "opencode",
    "openai",
    "openrouter",
    "ollama",
    "groq",
    "custom",
];

pub async fn presets() -> Json<&'static [crate::domain::ProviderPreset]> {
    Json(PROVIDER_PRESETS)
}

#[derive(Debug, Deserialize)]
pub struct CreateProvider {
    pub name: String,
    pub preset: Option<String>,
    pub base_url: String,
    /// Name of an existing secret (preferred) — mutually exclusive with api_key.
    pub secret_name: Option<String>,
    /// Inline API key — wrapped into a secret automatically.
    pub api_key: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let providers = repo::providers::list_providers(&state.pool).await?;
    // Attach masked secret info.
    let mut out = Vec::new();
    for p in providers {
        let mut v = serde_json::to_value(&p)?;
        if let Some(sid) = p.secret_id {
            v["has_secret"] = serde_json::json!(true);
            v["secret_id"] = serde_json::json!(sid);
        } else {
            v["has_secret"] = serde_json::json!(false);
        }
        out.push(v);
    }
    Ok(Json(serde_json::json!({"providers": out})))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Json(payload): Json<CreateProvider>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    if payload.name.trim().is_empty() || payload.base_url.trim().is_empty() {
        return Err(AppError::validation("name and base_url are required"));
    }
    let preset = payload.preset.unwrap_or_else(|| "custom".into());
    if !VALID_PRESETS.contains(&preset.as_str()) {
        return Err(AppError::validation(format!("invalid preset: {preset}")));
    }
    let mut secret_id: Option<Uuid> = None;
    if let Some(key) = payload.api_key.as_deref().filter(|k| !k.is_empty()) {
        let name = payload.secret_name.clone().unwrap_or_else(|| {
            format!(
                "provider_{}_key",
                payload.name.trim().to_lowercase().replace(' ', "_")
            )
        });
        let secret = crate::secrets::SecretService::create(
            &state.pool,
            &state.cfg.app_secret,
            &state.secrets,
            &name,
            &format!("API key for provider {}", payload.name),
            key,
            admin.0.id,
        )
        .await?;
        secret_id = Some(secret.id);
    } else if let Some(secret_name) = &payload.secret_name {
        let row = sqlx::query_scalar::<_, Uuid>("SELECT id FROM secrets WHERE name = $1")
            .bind(secret_name)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found(format!("secret '{secret_name}' not found")))?;
        secret_id = Some(row);
    }

    let provider = repo::providers::create_provider(
        &state.pool,
        payload.name.trim(),
        &preset,
        payload.base_url.trim(),
        secret_id,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(&provider)?)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateProvider {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub status: Option<String>,
    /// Rotate the key: inline value (creates/updates a secret) or a secret name.
    pub api_key: Option<String>,
    pub secret_name: Option<String>,
}

pub async fn update(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Json(payload): Json<UpdateProvider>,
) -> AppResult<Json<serde_json::Value>> {
    let existing = repo::providers::provider_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("provider not found"))?;

    // Handle key rotation before the generic update.
    let mut secret_id_change: Option<Option<Uuid>> = None;
    if let Some(key) = payload.api_key.as_deref().filter(|k| !k.is_empty()) {
        match existing.secret_id {
            Some(sid) => {
                crate::secrets::SecretService::update(
                    &state.pool,
                    &state.cfg.app_secret,
                    &state.secrets,
                    sid,
                    None,
                    Some(key),
                )
                .await?;
            }
            None => {
                let name = format!(
                    "provider_{}_key",
                    existing.name.to_lowercase().replace(' ', "_")
                );
                let secret = crate::secrets::SecretService::create(
                    &state.pool,
                    &state.cfg.app_secret,
                    &state.secrets,
                    &name,
                    &format!("API key for provider {}", existing.name),
                    key,
                    admin.0.id,
                )
                .await?;
                secret_id_change = Some(Some(secret.id));
            }
        }
    } else if let Some(secret_name) = &payload.secret_name {
        let sid = sqlx::query_scalar::<_, Uuid>("SELECT id FROM secrets WHERE name = $1")
            .bind(secret_name)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found(format!("secret '{secret_name}' not found")))?;
        secret_id_change = Some(Some(sid));
    }

    let provider = repo::providers::update_provider(
        &state.pool,
        id,
        payload.name.as_deref(),
        payload.base_url.as_deref(),
        secret_id_change,
        payload.status.as_deref(),
    )
    .await?;
    Ok(Json(serde_json::to_value(&provider)?))
}

pub async fn delete(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<StatusCode> {
    repo::providers::delete_provider(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Models ----------

#[derive(Debug, Deserialize)]
pub struct CreateModel {
    /// Optional on the provider-scoped route (taken from the path).
    pub provider_id: Option<Uuid>,
    pub api_name: String,
    pub display_name: Option<String>,
    pub context_window: Option<i32>,
    #[serde(default)]
    pub capabilities: serde_json::Value,
    /// Default reasoning effort (`low|medium|high`); absent/empty = provider default.
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

pub async fn list_models(
    Path(provider_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let models = repo::providers::list_models(&state.pool, Some(provider_id)).await?;
    Ok(Json(serde_json::json!({"models": models})))
}

pub async fn create_model(
    Path(provider_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(mut payload): Json<CreateModel>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    if payload.api_name.trim().is_empty() {
        return Err(AppError::validation("api_name is required"));
    }
    let provider_id = payload.provider_id.take().unwrap_or(provider_id);
    repo::providers::provider_by_id(&state.pool, provider_id)
        .await?
        .ok_or_else(|| AppError::not_found("provider not found"))?;
    let reasoning_effort = match payload.reasoning_effort.as_deref() {
        Some(raw) => crate::domain::normalize_effort(raw).map_err(AppError::validation)?,
        None => None,
    };
    let model = repo::providers::create_model(
        &state.pool,
        provider_id,
        payload.api_name.trim(),
        payload.display_name.unwrap_or_default().trim(),
        payload.context_window,
        payload.capabilities,
        reasoning_effort.as_deref(),
        payload.is_default,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(&model)?)))
}

/// `Option<Option<T>>` that maps JSON `null` → `Some(None)` (explicit clear).
/// Serde's default collapses `null` to the outer `None`, which would mean
/// "keep the existing value" in our tri-state update contract.
mod null_means_clear {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Ok(Some(Option::<T>::deserialize(de)?))
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateModel {
    pub api_name: Option<String>,
    pub display_name: Option<String>,
    pub context_window: Option<Option<i32>>,
    pub capabilities: Option<serde_json::Value>,
    /// `null` clears (provider default); a string sets it; absent keeps it.
    #[serde(default, deserialize_with = "null_means_clear::deserialize")]
    pub reasoning_effort: Option<Option<String>>,
    pub is_default: Option<bool>,
    pub status: Option<String>,
}

pub async fn update_model(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<UpdateModel>,
) -> AppResult<Json<serde_json::Value>> {
    let api_name = payload.api_name.as_deref().map(str::trim);
    if api_name.is_some_and(str::is_empty) {
        return Err(AppError::validation("api_name must not be empty"));
    }
    // Absent → keep; null → clear; string → validate & normalize ("" = clear).
    let reasoning_effort = match payload.reasoning_effort {
        None => None,
        Some(raw_opt) => Some(match raw_opt.as_deref() {
            None => Ok(None),
            Some(raw) => crate::domain::normalize_effort(raw).map_err(AppError::validation),
        }?),
    };
    let model = repo::providers::update_model(
        &state.pool,
        id,
        api_name,
        payload.display_name.as_deref(),
        payload.context_window,
        payload.capabilities,
        reasoning_effort,
        payload.is_default,
        payload.status.as_deref(),
    )
    .await?;
    Ok(Json(serde_json::to_value(&model)?))
}

pub async fn delete_model(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<StatusCode> {
    repo::providers::delete_model(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Model discovery ----------

/// Resolve a provider's adapter + credentials (used by both chat and discovery).
async fn provider_credentials(
    state: &AppState,
    provider: &crate::domain::Provider,
) -> AppResult<(
    Arc<dyn crate::llm::Provider>,
    crate::llm::ProviderCredentials,
)> {
    let adapter = state
        .providers
        .get("openaicompat")
        .cloned()
        .ok_or_else(|| AppError::internal("openaicompat provider missing from registry"))?;
    let api_key = match provider.secret_id {
        Some(secret_id) => {
            let name = sqlx::query_scalar::<_, String>("SELECT name FROM secrets WHERE id = $1")
                .bind(secret_id)
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| {
                    AppError::bad_request("provider secret was deleted — reconfigure the provider")
                })?;
            Some(state.secrets.get(&name).await?)
        }
        None => None,
    };
    Ok((
        adapter,
        crate::llm::ProviderCredentials {
            base_url: provider.base_url.clone(),
            api_key,
        },
    ))
}

/// `GET {base_url}/models` on the provider, mapped against what is already
/// configured so the admin UI can offer a one-click "add missing models".
pub async fn discover_models(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let provider = repo::providers::provider_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("provider not found"))?;
    let (adapter, credentials) = provider_credentials(&state, &provider).await?;

    let discovered = adapter
        .list_models(&credentials)
        .await
        .map_err(|e| AppError::bad_request(format!("model discovery failed: {e}")))?;

    let existing: std::collections::HashSet<String> =
        repo::providers::list_models(&state.pool, Some(id))
            .await?
            .into_iter()
            .map(|m| m.api_name)
            .collect();

    let mut models: Vec<_> = discovered
        .into_iter()
        .map(|m| {
            let configured = existing.contains(&m.api_name);
            serde_json::json!({
                "api_name": m.api_name,
                "display_name": m.display_name,
                "context_window": m.context_window,
                "configured": configured,
            })
        })
        .collect();
    models.sort_by(|a, b| {
        a["api_name"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["api_name"].as_str().unwrap_or_default())
    });

    Ok(Json(serde_json::json!({ "models": models })))
}

#[derive(Debug, Deserialize)]
pub struct ImportModels {
    /// Wire names to add (ignored silently when already configured).
    pub api_names: Vec<String>,
}

/// Bulk-add discovered models. Existing `api_name`s are skipped, so this is
/// idempotent and safe to re-run after a provider publishes new models.
pub async fn import_models(
    Path(provider_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(payload): Json<ImportModels>,
) -> AppResult<Json<serde_json::Value>> {
    repo::providers::provider_by_id(&state.pool, provider_id)
        .await?
        .ok_or_else(|| AppError::not_found("provider not found"))?;

    let names: Vec<String> = payload
        .api_names
        .into_iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return Err(AppError::validation("api_names must not be empty"));
    }

    // Re-read discovery so display name + context window come from the
    // provider instead of being lost between the two calls. A discovery
    // failure is not fatal: fall back to bare api names.
    let provider = repo::providers::provider_by_id(&state.pool, provider_id)
        .await?
        .ok_or_else(|| AppError::not_found("provider not found"))?;
    let discovered = match provider_credentials(&state, &provider).await {
        Ok((adapter, credentials)) => adapter.list_models(&credentials).await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let rows: Vec<(String, String, Option<i32>)> = names
        .iter()
        .map(|name| {
            let found = discovered.iter().find(|d| &d.api_name == name);
            (
                name.clone(),
                found
                    .map(|d| d.display_name.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| name.clone()),
                found.and_then(|d| d.context_window),
            )
        })
        .collect();

    let created = repo::providers::import_models(&state.pool, provider_id, &rows).await?;
    Ok(Json(serde_json::json!({
        "created": created,
        "created_count": created.len(),
        "skipped_count": names.len() - created.len(),
    })))
}
