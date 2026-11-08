use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::Approval;
use crate::error::AppResult;

#[allow(clippy::too_many_arguments)]
pub async fn create_pending(
    pool: &PgPool,
    conversation_id: Uuid,
    tool_call_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    reason: &str,
    requested_resource: Option<&str>,
) -> AppResult<Approval> {
    let row = sqlx::query_as::<_, Approval>(
        r#"INSERT INTO approvals (conversation_id, tool_call_id, tool_name, input, reason, requested_resource)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (conversation_id, tool_call_id) DO UPDATE SET input = $4, reason = $5, requested_resource = $6
         RETURNING *"#,
    )
    .bind(conversation_id)
    .bind(tool_call_id)
    .bind(tool_name)
    .bind(input)
    .bind(reason)
    .bind(requested_resource)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn by_conversation_and_call(
    pool: &PgPool,
    conversation_id: Uuid,
    tool_call_id: &str,
) -> AppResult<Option<Approval>> {
    let row = sqlx::query_as::<_, Approval>(
        "SELECT * FROM approvals WHERE conversation_id = $1 AND tool_call_id = $2",
    )
    .bind(conversation_id)
    .bind(tool_call_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<Approval>> {
    let row = sqlx::query_as::<_, Approval>("SELECT * FROM approvals WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn decide(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    decision_mode: Option<&str>,
) -> AppResult<Approval> {
    let row = sqlx::query_as::<_, Approval>(
        "UPDATE approvals SET status = $2, decision_mode = $3, decided_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(status)
    .bind(decision_mode)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn list_for_conversation(
    pool: &PgPool,
    conversation_id: Uuid,
) -> AppResult<Vec<Approval>> {
    let rows = sqlx::query_as::<_, Approval>(
        "SELECT * FROM approvals WHERE conversation_id = $1 ORDER BY created_at",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
