use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::ToolExecution;
use crate::error::AppResult;

#[allow(clippy::too_many_arguments)]
pub async fn insert(
    pool: &PgPool,
    run_id: Option<Uuid>,
    conversation_id: Uuid,
    tool_call_id: &str,
    tool_id: Option<Uuid>,
    tool_name: &str,
    input: Value,
    output: Option<Value>,
    status: &str,
    error_text: Option<&str>,
    duration_ms: Option<i32>,
    resources_used: Option<Value>,
) -> AppResult<ToolExecution> {
    let row = sqlx::query_as::<_, ToolExecution>(
        r#"INSERT INTO tool_executions
           (run_id, conversation_id, tool_call_id, tool_id, tool_name, input, output, status, error_text, duration_ms, resources_used)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
           RETURNING *"#,
    )
    .bind(run_id)
    .bind(conversation_id)
    .bind(tool_call_id)
    .bind(tool_id)
    .bind(tool_name)
    .bind(input)
    .bind(output)
    .bind(status)
    .bind(error_text)
    .bind(duration_ms)
    .bind(resources_used)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn by_conversation_and_tool_call(
    pool: &PgPool,
    conversation_id: Uuid,
    tool_call_id: &str,
) -> AppResult<Option<ToolExecution>> {
    let row = sqlx::query_as::<_, ToolExecution>(
        "SELECT * FROM tool_executions WHERE conversation_id = $1 AND tool_call_id = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(conversation_id)
    .bind(tool_call_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_for_tool(
    pool: &PgPool,
    tool_id: Uuid,
    limit: i64,
) -> AppResult<Vec<ToolExecution>> {
    let rows = sqlx::query_as::<_, ToolExecution>(
        "SELECT * FROM tool_executions WHERE tool_id = $1 ORDER BY created_at DESC LIMIT $2",
    )
    .bind(tool_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_for_conversation(
    pool: &PgPool,
    conversation_id: Uuid,
    limit: i64,
) -> AppResult<Vec<ToolExecution>> {
    let rows = sqlx::query_as::<_, ToolExecution>(
        "SELECT * FROM tool_executions WHERE conversation_id = $1 ORDER BY created_at DESC LIMIT $2",
    )
    .bind(conversation_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
