use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::api::AuthUser;
use crate::domain::PublicUser;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::service::auth as auth_service;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8, max = 128))]
    pub password: String,
    #[validate(length(max = 100))]
    pub name: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 1, max = 128))]
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: PublicUser,
}

#[derive(Debug, Serialize)]
pub struct MetaResponse {
    pub registration_enabled: bool,
}

/// Public metadata the SPA reads before login (e.g. whether to show "Sign up").
pub async fn meta(State(state): State<Arc<AppState>>) -> Json<MetaResponse> {
    Json(MetaResponse {
        registration_enabled: state.cfg.registration_enabled,
    })
}

fn validate_or_422<T: Validate>(payload: &T) -> AppResult<()> {
    payload.validate().map_err(|e| {
        let msgs: Vec<String> = e
            .field_errors()
            .into_iter()
            .flat_map(|(field, errors)| {
                errors
                    .iter()
                    .map(|err| format!("{field}: {}", err.code))
                    .collect::<Vec<_>>()
            })
            .collect();
        AppError::validation(msgs.join("; "))
    })
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> AppResult<(StatusCode, Json<AuthResponse>)> {
    if !state.cfg.registration_enabled {
        return Err(AppError::forbidden("registration is disabled"));
    }
    validate_or_422(&payload)?;
    let first_user = repo::users::count(&state.pool).await? == 0;
    let role = if first_user { "admin" } else { "user" };
    let hash = auth_service::hash_password(&payload.password)?;
    let user = repo::users::create(&state.pool, &payload.email, &hash, &payload.name, role).await?;
    repo::users::ensure_preferences(&state.pool, user.id).await?;
    let token = auth_service::create_token(&state.cfg.jwt_secret, user.id, &user.role)?;
    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            token,
            user: PublicUser::from(&user),
        }),
    ))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> AppResult<Json<AuthResponse>> {
    validate_or_422(&payload)?;
    let user = repo::users::by_email(&state.pool, &payload.email)
        .await?
        .ok_or_else(|| AppError::unauthorized("invalid email or password"))?;
    if !auth_service::verify_password(&payload.password, &user.password_hash) {
        return Err(AppError::unauthorized("invalid email or password"));
    }
    let token = auth_service::create_token(&state.cfg.jwt_secret, user.id, &user.role)?;
    Ok(Json(AuthResponse {
        token,
        user: PublicUser::from(&user),
    }))
}

pub async fn me(State(state): State<Arc<AppState>>, user: AuthUser) -> AppResult<Json<PublicUser>> {
    let u = repo::users::by_id(&state.pool, user.id)
        .await?
        .ok_or_else(|| AppError::unauthorized("account no longer exists"))?;
    Ok(Json(PublicUser::from(&u)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfile {
    pub name: Option<String>,
    /// Empty string clears the avatar.
    pub avatar_url: Option<String>,
}

pub async fn update_profile(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(payload): Json<UpdateProfile>,
) -> AppResult<Json<PublicUser>> {
    if payload
        .name
        .as_deref()
        .is_some_and(|n| n.chars().count() > 100)
    {
        return Err(AppError::validation("name must be at most 100 characters"));
    }
    if payload
        .avatar_url
        .as_deref()
        .is_some_and(|u| u.len() > 2048)
    {
        return Err(AppError::validation(
            "avatar_url must be at most 2048 characters",
        ));
    }
    let name = payload.name.as_deref().map(str::trim);
    // Some(trimmed) sets the avatar; Some("") clears it; absent keeps it.
    let avatar_url = payload.avatar_url.as_deref().map(|raw| {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    });
    let updated = repo::users::update_profile(&state.pool, user.id, name, avatar_url).await?;
    Ok(Json(PublicUser::from(&updated)))
}
