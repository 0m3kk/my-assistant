use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::ConversationGroup;
use crate::error::{AppError, AppResult};

pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<ConversationGroup>> {
    let rows = sqlx::query_as::<_, ConversationGroup>(
        "SELECT * FROM conversation_groups WHERE user_id = $1 ORDER BY position, lower(name)",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Owned lookup — used to validate assignments before touching a conversation.
pub async fn owned_by_id(pool: &PgPool, id: Uuid, user_id: Uuid) -> AppResult<ConversationGroup> {
    let row = sqlx::query_as::<_, ConversationGroup>(
        "SELECT * FROM conversation_groups WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("folder not found"))?;
    Ok(row)
}

pub async fn create(pool: &PgPool, user_id: Uuid, name: &str) -> AppResult<ConversationGroup> {
    let row = sqlx::query_as::<_, ConversationGroup>(
        r#"INSERT INTO conversation_groups (user_id, name, position)
           VALUES ($1, $2, COALESCE(
               (SELECT MAX(position) + 1 FROM conversation_groups WHERE user_id = $1), 0
           ))
           RETURNING *"#,
    )
    .bind(user_id)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(map_conflict)?;
    Ok(row)
}

pub async fn rename(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    name: &str,
) -> AppResult<ConversationGroup> {
    let row = sqlx::query_as::<_, ConversationGroup>(
        "UPDATE conversation_groups SET name = $3, updated_at = now() \
         WHERE id = $1 AND user_id = $2 RETURNING *",
    )
    .bind(id)
    .bind(user_id)
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(map_conflict)?
    .ok_or_else(|| AppError::not_found("folder not found"))?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM conversation_groups WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("folder not found"));
    }
    Ok(())
}

fn map_conflict(e: sqlx::Error) -> AppError {
    match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            AppError::conflict("a folder with that name already exists")
        }
        other => other.into(),
    }
}
