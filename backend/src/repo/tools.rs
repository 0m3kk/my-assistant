use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::ToolRow;
use crate::error::{AppError, AppResult};

pub async fn list(pool: &PgPool) -> AppResult<Vec<ToolRow>> {
    let rows = sqlx::query_as::<_, ToolRow>("SELECT * FROM tools ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn list_visible(pool: &PgPool) -> AppResult<Vec<ToolRow>> {
    let rows =
        sqlx::query_as::<_, ToolRow>("SELECT * FROM tools WHERE is_meta = FALSE ORDER BY name")
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<ToolRow>> {
    let row = sqlx::query_as::<_, ToolRow>("SELECT * FROM tools WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn by_name(pool: &PgPool, name: &str) -> AppResult<Option<ToolRow>> {
    let row = sqlx::query_as::<_, ToolRow>("SELECT * FROM tools WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &PgPool,
    name: &str,
    display_name: &str,
    description: &str,
    kind: &str,
    source_ref: &str,
    config: serde_json::Value,
    parameters_schema: serde_json::Value,
    requires_approval: bool,
    is_meta: bool,
    is_builtin: bool,
    created_by: Option<Uuid>,
) -> AppResult<ToolRow> {
    let row = sqlx::query_as::<_, ToolRow>(
        r#"INSERT INTO tools
           (name, display_name, description, kind, source_ref, config, parameters_schema,
            requires_approval, is_meta, is_builtin, created_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING *"#,
    )
    .bind(name)
    .bind(display_name)
    .bind(description)
    .bind(kind)
    .bind(source_ref)
    .bind(config)
    .bind(parameters_schema)
    .bind(requires_approval)
    .bind(is_meta)
    .bind(is_builtin)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            AppError::conflict("tool name already exists")
        }
        other => other.into(),
    })?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    display_name: Option<&str>,
    description: Option<&str>,
    config: Option<serde_json::Value>,
    parameters_schema: Option<serde_json::Value>,
    requires_approval: Option<bool>,
    status: Option<&str>,
    group_id: Option<Option<Uuid>>,
) -> AppResult<ToolRow> {
    let existing = by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("tool not found"))?;
    let row = sqlx::query_as::<_, ToolRow>(
        r#"UPDATE tools SET
            display_name = $2, description = $3,
            config = COALESCE($4, config),
            parameters_schema = COALESCE($5, parameters_schema),
            requires_approval = COALESCE($6, requires_approval),
            status = $7,
            group_id = CASE WHEN $8::boolean THEN $9::uuid ELSE group_id END,
            revision = revision + 1,
            updated_at = now()
         WHERE id = $1 RETURNING *"#,
    )
    .bind(id)
    .bind(display_name.unwrap_or(&existing.display_name))
    .bind(description.unwrap_or(&existing.description))
    .bind(config)
    .bind(parameters_schema)
    .bind(requires_approval)
    .bind(status.unwrap_or(&existing.status))
    .bind(group_id.is_some())
    .bind(group_id.flatten())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let existing = by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("tool not found"))?;
    if existing.is_builtin {
        return Err(AppError::conflict("built-in tools cannot be deleted"));
    }
    sqlx::query("DELETE FROM tools WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn snapshot(pool: &PgPool, id: Uuid) -> AppResult<serde_json::Value> {
    let tool = by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("tool not found"))?;
    Ok(serde_json::to_value(&tool).unwrap_or_default())
}
