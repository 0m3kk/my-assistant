use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct VersionRow {
    pub revision: i32,
    pub snapshot: Value,
    pub note: String,
    pub created_at: DateTime<Utc>,
}

pub async fn insert_tool_version(
    pool: &PgPool,
    tool_id: Uuid,
    revision: i32,
    snapshot: Value,
    created_by: Option<Uuid>,
    proposal_id: Option<Uuid>,
    note: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO tool_versions (tool_id, revision, snapshot, created_by, proposal_id, note) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (tool_id, revision) DO NOTHING",
    )
    .bind(tool_id)
    .bind(revision)
    .bind(snapshot)
    .bind(created_by)
    .bind(proposal_id)
    .bind(note)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_agent_version(
    pool: &PgPool,
    agent_id: Uuid,
    revision: i32,
    snapshot: Value,
    created_by: Option<Uuid>,
    proposal_id: Option<Uuid>,
    note: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO agent_versions (agent_id, revision, snapshot, created_by, proposal_id, note) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (agent_id, revision) DO NOTHING",
    )
    .bind(agent_id)
    .bind(revision)
    .bind(snapshot)
    .bind(created_by)
    .bind(proposal_id)
    .bind(note)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_tool_versions(pool: &PgPool, tool_id: Uuid) -> AppResult<Vec<VersionRow>> {
    let rows = sqlx::query_as::<_, VersionRow>(
        "SELECT revision, snapshot, note, created_at FROM tool_versions WHERE tool_id = $1 ORDER BY revision DESC",
    )
    .bind(tool_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_agent_versions(pool: &PgPool, agent_id: Uuid) -> AppResult<Vec<VersionRow>> {
    let rows = sqlx::query_as::<_, VersionRow>(
        "SELECT revision, snapshot, note, created_at FROM agent_versions WHERE agent_id = $1 ORDER BY revision DESC",
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn tool_version(
    pool: &PgPool,
    tool_id: Uuid,
    revision: i32,
) -> AppResult<Option<VersionRow>> {
    let row = sqlx::query_as::<_, VersionRow>(
        "SELECT revision, snapshot, note, created_at FROM tool_versions WHERE tool_id = $1 AND revision = $2",
    )
    .bind(tool_id)
    .bind(revision)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn agent_version(
    pool: &PgPool,
    agent_id: Uuid,
    revision: i32,
) -> AppResult<Option<VersionRow>> {
    let row = sqlx::query_as::<_, VersionRow>(
        "SELECT revision, snapshot, note, created_at FROM agent_versions WHERE agent_id = $1 AND revision = $2",
    )
    .bind(agent_id)
    .bind(revision)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
