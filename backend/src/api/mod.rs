pub mod admin;
pub mod agents;
pub mod approvals;
pub mod auth;
pub mod chat;
pub mod conversation_groups;
pub mod conversations;
pub mod feedback;
pub mod files;
pub mod models;
pub mod preferences;
pub mod workspace;

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Authenticated caller (JWT bearer).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: String,
}

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> AppResult<Self> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("missing Authorization header"))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::unauthorized("Authorization header must be a Bearer token"))?;
        let claims = crate::service::auth::verify_token(&state.cfg.jwt_secret, token)?;
        // Confirm the account still exists.
        let exists = crate::repo::users::by_id(&state.pool, claims.sub).await?;
        let Some(u) = exists else {
            return Err(AppError::unauthorized("account no longer exists"));
        };
        Ok(AuthUser {
            id: u.id,
            role: u.role,
        })
    }
}

/// Admin-only caller.
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> AppResult<Self> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.role != "admin" {
            return Err(AppError::forbidden("admin role required"));
        }
        Ok(AdminUser(user))
    }
}
