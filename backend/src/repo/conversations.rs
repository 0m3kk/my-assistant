use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{Conversation, MessageRow};
use crate::error::{AppError, AppResult};

// ---------- Conversations ----------

pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<Conversation>> {
    let rows = sqlx::query_as::<_, Conversation>(
        "SELECT * FROM conversations WHERE user_id = $1 ORDER BY COALESCE(last_message_at, created_at) DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[allow(dead_code)]
pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<Conversation>> {
    let row = sqlx::query_as::<_, Conversation>("SELECT * FROM conversations WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Owned lookup — enforces user scoping for all conversation access.
pub async fn owned_by_id(pool: &PgPool, id: Uuid, user_id: Uuid) -> AppResult<Conversation> {
    let row = sqlx::query_as::<_, Conversation>(
        "SELECT * FROM conversations WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("conversation not found"))?;
    Ok(row)
}

pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    agent_id: Uuid,
    title: &str,
    model_id: Option<Uuid>,
    group_id: Option<Uuid>,
) -> AppResult<Conversation> {
    let row = sqlx::query_as::<_, Conversation>(
        "INSERT INTO conversations (user_id, agent_id, title, model_id, group_id) VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(user_id)
    .bind(agent_id)
    .bind(title)
    .bind(model_id)
    .bind(group_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Create with a client-generated id (used when the UI pre-allocates the
/// conversation uuid so the chat instance is stable from the first message).
pub async fn create_with_id(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    agent_id: Uuid,
    title: &str,
    model_id: Option<Uuid>,
    group_id: Option<Uuid>,
) -> AppResult<Conversation> {
    let row = sqlx::query_as::<_, Conversation>(
        "INSERT INTO conversations (id, user_id, agent_id, title, model_id, group_id) VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(id)
    .bind(user_id)
    .bind(agent_id)
    .bind(title)
    .bind(model_id)
    .bind(group_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn set_status(pool: &PgPool, id: Uuid, status: &str) -> AppResult<()> {
    sqlx::query("UPDATE conversations SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn rename(pool: &PgPool, id: Uuid, title: &str) -> AppResult<Conversation> {
    let row = sqlx::query_as::<_, Conversation>(
        "UPDATE conversations SET title = $2, title_generated = TRUE, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(title)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Move a conversation into a folder, or out of any folder (`None`).
pub async fn set_group(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    group_id: Option<Uuid>,
) -> AppResult<Conversation> {
    let row = sqlx::query_as::<_, Conversation>(
        "UPDATE conversations SET group_id = $3, updated_at = now() \
         WHERE id = $1 AND user_id = $2 RETURNING *",
    )
    .bind(id)
    .bind(user_id)
    .bind(group_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("conversation not found"))?;
    Ok(row)
}

/// Apply an automatically generated title, but only while the conversation is
/// still using the first-message heuristic. Returns `None` when the title was
/// already generated or manually renamed (so the update is a no-op).
pub async fn set_generated_title(
    pool: &PgPool,
    id: Uuid,
    title: &str,
) -> AppResult<Option<Conversation>> {
    let row = sqlx::query_as::<_, Conversation>(
        "UPDATE conversations SET title = $2, title_generated = TRUE, updated_at = now() \
         WHERE id = $1 AND title_generated = FALSE RETURNING *",
    )
    .bind(id)
    .bind(title)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[allow(dead_code)]
pub async fn set_model(pool: &PgPool, id: Uuid, model_id: Option<Uuid>) -> AppResult<Conversation> {
    let row = sqlx::query_as::<_, Conversation>(
        "UPDATE conversations SET model_id = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(model_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM conversations WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("conversation not found"));
    }
    Ok(())
}

// ---------- Messages ----------

pub async fn messages(pool: &PgPool, conversation_id: Uuid) -> AppResult<Vec<MessageRow>> {
    let rows = sqlx::query_as::<_, MessageRow>(
        "SELECT * FROM messages WHERE conversation_id = $1 ORDER BY seq",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn next_seq(pool: &PgPool, conversation_id: Uuid) -> AppResult<i32> {
    let seq: Option<i32> =
        sqlx::query_scalar("SELECT max(seq) FROM messages WHERE conversation_id = $1")
            .bind(conversation_id)
            .fetch_one(pool)
            .await?;
    Ok(seq.unwrap_or(0) + 1)
}

pub async fn append_message(
    pool: &PgPool,
    conversation_id: Uuid,
    run_id: Option<Uuid>,
    role: &str,
    parts: serde_json::Value,
    model_id: Option<Uuid>,
) -> AppResult<MessageRow> {
    let seq = next_seq(pool, conversation_id).await?;
    let row = sqlx::query_as::<_, MessageRow>(
        r#"INSERT INTO messages (conversation_id, run_id, role, parts, model_id, seq)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *"#,
    )
    .bind(conversation_id)
    .bind(run_id)
    .bind(role)
    .bind(parts)
    .bind(model_id)
    .bind(seq)
    .fetch_one(pool)
    .await?;
    sqlx::query("UPDATE conversations SET last_message_at = $2, updated_at = now() WHERE id = $1")
        .bind(conversation_id)
        .bind(Utc::now())
        .execute(pool)
        .await?;
    Ok(row)
}

/// Persist the assistant parts for a message that was just created
/// (insert-if-new, update-if-existing by seq conversation+message id).
pub async fn upsert_message_parts(
    pool: &PgPool,
    message_id: Uuid,
    parts: serde_json::Value,
    model_id: Option<Uuid>,
    usage: Option<serde_json::Value>,
) -> AppResult<()> {
    sqlx::query("UPDATE messages SET parts = $2, model_id = COALESCE($3, model_id), usage = COALESCE($4, usage) WHERE id = $1")
        .bind(message_id)
        .bind(parts)
        .bind(model_id)
        .bind(usage)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update a single tool part's state/output inside a stored message
/// (used when an approved tool executes on a later request).
pub async fn update_tool_part(
    pool: &PgPool,
    message_id: Uuid,
    tool_call_id: &str,
    state: &str,
    output: Option<serde_json::Value>,
    error_text: Option<&str>,
) -> AppResult<bool> {
    let row: Option<MessageRow> =
        sqlx::query_as::<_, MessageRow>("SELECT * FROM messages WHERE id = $1")
            .bind(message_id)
            .fetch_optional(pool)
            .await?;
    let Some(row) = row else { return Ok(false) };
    let mut parts = row.parts.clone();
    let Some(arr) = parts.as_array_mut() else {
        return Ok(false);
    };
    let mut updated = false;
    for part in arr.iter_mut() {
        let is_target = part.get("toolCallId").and_then(|v| v.as_str()) == Some(tool_call_id)
            && part
                .get("type")
                .and_then(|v| v.as_str())
                .is_some_and(|t| t.starts_with("tool-"));
        if is_target {
            part["state"] = serde_json::json!(state);
            if let Some(o) = &output {
                part["output"] = o.clone();
            }
            if let Some(e) = error_text {
                part["errorText"] = serde_json::json!(e);
            }
            updated = true;
        }
    }
    if updated {
        sqlx::query("UPDATE messages SET parts = $2 WHERE id = $1")
            .bind(message_id)
            .bind(parts)
            .execute(pool)
            .await?;
    }
    Ok(updated)
}

#[allow(dead_code)]
pub async fn message_by_id(pool: &PgPool, message_id: Uuid) -> AppResult<Option<MessageRow>> {
    let row = sqlx::query_as::<_, MessageRow>("SELECT * FROM messages WHERE id = $1")
        .bind(message_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Find a stored message that contains a tool part with the given call id.
/// Uses jsonb containment so the match is independent of jsonb text formatting.
/// tool_call_id is server-generated (`call_<hex>`).
pub async fn by_conversation_and_tool_call(
    pool: &PgPool,
    conversation_id: Uuid,
    tool_call_id: &str,
) -> AppResult<Option<MessageRow>> {
    let safe: String = tool_call_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if safe.is_empty() || safe != tool_call_id {
        return Ok(None);
    }
    let containment = serde_json::json!([{ "toolCallId": safe }]);
    let row = sqlx::query_as::<_, MessageRow>(
        "SELECT * FROM messages WHERE conversation_id = $1 AND parts @> $2 ORDER BY seq DESC LIMIT 1",
    )
    .bind(conversation_id)
    .bind(containment)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
