use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;

pub async fn insert(
    pool: &PgPool,
    user_id: Option<Uuid>,
    action: &str,
    target: &str,
    allowed: bool,
    detail: serde_json::Value,
) -> AppResult<()> {
    sqlx::query("INSERT INTO audit_logs (user_id, action, target, allowed, detail) VALUES ($1, $2, $3, $4, $5)")
        .bind(user_id)
        .bind(action)
        .bind(target)
        .bind(allowed)
        .bind(detail)
        .execute(pool)
        .await?;
    Ok(())
}
