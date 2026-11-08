use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::ToolBuild;
use crate::error::AppResult;

pub async fn create_pending(
    pool: &PgPool,
    tool_id: Uuid,
    revision: i32,
    language: &str,
    source: &str,
    runtime: &str,
    created_by: Option<Uuid>,
) -> AppResult<ToolBuild> {
    let row = sqlx::query_as::<_, ToolBuild>(
        r#"INSERT INTO tool_builds (tool_id, revision, language, source, runtime, created_by)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (tool_id, revision) DO UPDATE SET
             language = EXCLUDED.language,
             source = EXCLUDED.source,
             runtime = EXCLUDED.runtime,
             status = 'pending',
             build_log = NULL,
             finished_at = NULL
           RETURNING *"#,
    )
    .bind(tool_id)
    .bind(revision)
    .bind(language)
    .bind(source)
    .bind(runtime)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn by_tool_revision(
    pool: &PgPool,
    tool_id: Uuid,
    revision: i32,
) -> AppResult<Option<ToolBuild>> {
    let row = sqlx::query_as::<_, ToolBuild>(
        "SELECT * FROM tool_builds WHERE tool_id = $1 AND revision = $2",
    )
    .bind(tool_id)
    .bind(revision)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn latest(pool: &PgPool, tool_id: Uuid) -> AppResult<Option<ToolBuild>> {
    let row = sqlx::query_as::<_, ToolBuild>(
        "SELECT * FROM tool_builds WHERE tool_id = $1 ORDER BY revision DESC LIMIT 1",
    )
    .bind(tool_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn latest_built(pool: &PgPool, tool_id: Uuid) -> AppResult<Option<ToolBuild>> {
    let row = sqlx::query_as::<_, ToolBuild>(
        "SELECT * FROM tool_builds WHERE tool_id = $1 AND status = 'built' ORDER BY revision DESC LIMIT 1",
    )
    .bind(tool_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_for_tool(pool: &PgPool, tool_id: Uuid) -> AppResult<Vec<ToolBuild>> {
    let rows = sqlx::query_as::<_, ToolBuild>(
        "SELECT * FROM tool_builds WHERE tool_id = $1 ORDER BY revision DESC",
    )
    .bind(tool_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_building(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE tool_builds SET status = 'building' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn mark_built(
    pool: &PgPool,
    id: Uuid,
    storage_key: &str,
    sha256: &str,
    size_bytes: i64,
    build_log: &str,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE tool_builds SET status = 'built', storage_key = $2, sha256 = $3, size_bytes = $4, build_log = $5, finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(storage_key)
    .bind(sha256)
    .bind(size_bytes)
    .bind(build_log)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_failed(pool: &PgPool, id: Uuid, build_log: &str) -> AppResult<()> {
    sqlx::query(
        "UPDATE tool_builds SET status = 'failed', build_log = $2, finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(build_log)
    .execute(pool)
    .await?;
    Ok(())
}
