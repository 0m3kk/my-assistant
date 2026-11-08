use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::ToolGroup;
use crate::error::{AppError, AppResult};

pub async fn list(pool: &PgPool) -> AppResult<Vec<ToolGroup>> {
    let rows =
        sqlx::query_as::<_, ToolGroup>("SELECT * FROM tool_groups ORDER BY position, lower(name)")
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<ToolGroup>> {
    let row = sqlx::query_as::<_, ToolGroup>("SELECT * FROM tool_groups WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn create(pool: &PgPool, name: &str) -> AppResult<ToolGroup> {
    let row = sqlx::query_as::<_, ToolGroup>(
        r#"INSERT INTO tool_groups (name, position)
           VALUES ($1, COALESCE((SELECT MAX(position) + 1 FROM tool_groups), 0))
           RETURNING *"#,
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(map_conflict)?;
    Ok(row)
}

pub async fn rename(pool: &PgPool, id: Uuid, name: &str) -> AppResult<ToolGroup> {
    let row = sqlx::query_as::<_, ToolGroup>(
        "UPDATE tool_groups SET name = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(map_conflict)?
    .ok_or_else(|| AppError::not_found("folder not found"))?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM tool_groups WHERE id = $1")
        .bind(id)
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
