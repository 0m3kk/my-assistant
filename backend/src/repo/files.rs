use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::FileRow;
use crate::error::{AppError, AppResult};

pub async fn insert(pool: &PgPool, row: NewFile) -> AppResult<FileRow> {
    let row = sqlx::query_as::<_, FileRow>(
        r#"INSERT INTO files (id, conversation_id, message_id, user_id, origin, filename, content_type, size_bytes, storage_key, checksum_sha256)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *"#,
    )
    .bind(row.id)
    .bind(row.conversation_id)
    .bind(row.message_id)
    .bind(row.user_id)
    .bind(row.origin)
    .bind(row.filename)
    .bind(row.content_type)
    .bind(row.size_bytes)
    .bind(row.storage_key)
    .bind(row.checksum_sha256)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Values for [`insert`] — the caller owns id/key generation.
pub struct NewFile {
    pub id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub message_id: Option<Uuid>,
    pub user_id: Uuid,
    pub origin: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub storage_key: String,
    pub checksum_sha256: Option<String>,
}

/// Owned lookup — enforces user scoping for all file access.
pub async fn owned_by_id(pool: &PgPool, id: Uuid, user_id: Uuid) -> AppResult<FileRow> {
    let row = sqlx::query_as::<_, FileRow>("SELECT * FROM files WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("file not found"))?;
    Ok(row)
}

/// Link a pending attachment to its conversation + message. Idempotent:
/// re-posted transcripts (approval resume) hit the same link again.
/// Returns false when the file was already linked to a different message.
pub async fn link(
    pool: &PgPool,
    id: Uuid,
    conversation_id: Uuid,
    message_id: Uuid,
) -> AppResult<bool> {
    let result = sqlx::query(
        "UPDATE files SET conversation_id = $2, message_id = $3
         WHERE id = $1 AND (conversation_id IS NULL OR conversation_id = $2)",
    )
    .bind(id)
    .bind(conversation_id)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM files WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// All files referenced by the given conversations (attachment resolution for
/// history assembly).
#[allow(dead_code)]
pub async fn by_conversation(pool: &PgPool, conversation_id: Uuid) -> AppResult<Vec<FileRow>> {
    let rows = sqlx::query_as::<_, FileRow>(
        "SELECT * FROM files WHERE conversation_id = $1 ORDER BY created_at ASC",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
