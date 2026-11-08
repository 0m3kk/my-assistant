use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::User;
use crate::error::{AppError, AppResult};

/// Projection for CLI listings — no password hash.
#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct UserListItem {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

pub async fn count(pool: &PgPool) -> AppResult<i64> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(n)
}

pub async fn count_admins(pool: &PgPool) -> AppResult<i64> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await?;
    Ok(n)
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<UserListItem>> {
    let users = sqlx::query_as::<_, UserListItem>(
        "SELECT id, email, name, role, created_at FROM users ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

pub async fn by_email(pool: &PgPool, email: &str) -> AppResult<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE lower(email) = lower($1)")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn create(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
    name: &str,
    role: &str,
) -> AppResult<User> {
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, password_hash, name, role) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(email.trim().to_lowercase())
    .bind(password_hash)
    .bind(name)
    .bind(role)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            AppError::conflict("an account with this email already exists")
        }
        other => other.into(),
    })?;
    Ok(user)
}

pub async fn ensure_preferences(pool: &PgPool, user_id: Uuid) -> AppResult<()> {
    sqlx::query("INSERT INTO user_preferences (user_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_profile(
    pool: &PgPool,
    user_id: Uuid,
    name: Option<&str>,
    avatar_url: Option<Option<&str>>,
) -> AppResult<User> {
    let existing = by_id(pool, user_id)
        .await?
        .ok_or_else(|| AppError::not_found("user not found"))?;
    let avatar_url = match avatar_url {
        None => existing.avatar_url,
        Some(value) => value.map(str::to_string),
    };
    let user = sqlx::query_as::<_, User>(
        "UPDATE users SET name = $2, avatar_url = $3, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(user_id)
    .bind(name.unwrap_or(&existing.name))
    .bind(avatar_url)
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn set_role(pool: &PgPool, user_id: Uuid, role: &str) -> AppResult<User> {
    let user = sqlx::query_as::<_, User>(
        "UPDATE users SET role = $1, updated_at = now() WHERE id = $2 RETURNING *",
    )
    .bind(role)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn delete(pool: &PgPool, user_id: Uuid) -> AppResult<u64> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
