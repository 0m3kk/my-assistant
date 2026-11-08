use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use crate::api::AuthUser;
use crate::domain::UserPreferences;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;

pub async fn get(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> AppResult<Json<UserPreferences>> {
    repo::users::ensure_preferences(&state.pool, user.id).await?;
    let prefs =
        sqlx::query_as::<_, UserPreferences>("SELECT * FROM user_preferences WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(prefs))
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdatePreferences {
    #[validate(custom(function = "validate_theme"))]
    pub theme: Option<String>,
    #[validate(custom(function = "validate_font_scale"))]
    pub font_scale: Option<String>,
    #[validate(length(min = 2, max = 10))]
    pub locale: Option<String>,
    /// `null` clears (use global default); a uuid sets it; absent keeps it.
    #[serde(default, deserialize_with = "null_means_clear::deserialize")]
    pub title_model_id: Option<Option<Uuid>>,
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

fn validate_theme(value: &str) -> Result<(), validator::ValidationError> {
    match value {
        "system" | "light" | "dark" => Ok(()),
        _ => Err(validator::ValidationError::new("invalid theme")),
    }
}

fn validate_font_scale(value: &str) -> Result<(), validator::ValidationError> {
    match value {
        "sm" | "md" | "lg" | "xl" => Ok(()),
        _ => Err(validator::ValidationError::new("invalid font scale")),
    }
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<UpdatePreferences>,
) -> AppResult<Json<UserPreferences>> {
    payload
        .validate()
        .map_err(|e| AppError::validation(format!("{e}")))?;
    repo::users::ensure_preferences(&state.pool, user.id).await?;

    if let Some(Some(model_id)) = payload.title_model_id {
        match repo::providers::model_by_id(&state.pool, model_id).await? {
            Some(m) if m.status == "enabled" => {}
            _ => return Err(AppError::validation("title model not found or disabled")),
        }
    }
    let (title_model_id, title_model_clear) = match payload.title_model_id {
        None => (None, false),
        Some(None) => (None, true),
        Some(Some(v)) => (Some(v), false),
    };

    let prefs = sqlx::query_as::<_, UserPreferences>(
        r#"UPDATE user_preferences SET
            theme = COALESCE($2, theme),
            font_scale = COALESCE($3, font_scale),
            locale = COALESCE($4, locale),
            title_model_id = CASE
                WHEN $5 THEN NULL
                ELSE COALESCE($6, title_model_id)
            END,
            updated_at = now()
         WHERE user_id = $1 RETURNING *"#,
    )
    .bind(user.id)
    .bind(payload.theme)
    .bind(payload.font_scale)
    .bind(payload.locale)
    .bind(title_model_clear)
    .bind(title_model_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(prefs))
}
