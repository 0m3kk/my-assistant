use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::Run;
use crate::error::AppResult;

pub async fn create(
    pool: &PgPool,
    conversation_id: Uuid,
    agent_id: Uuid,
    model_id: Option<Uuid>,
) -> AppResult<Run> {
    let row = sqlx::query_as::<_, Run>(
        "INSERT INTO runs (conversation_id, agent_id, model_id) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(conversation_id)
    .bind(agent_id)
    .bind(model_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn finish(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    assistant_message_id: Option<Uuid>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE runs SET status = $2, assistant_message_id = COALESCE($3, assistant_message_id), finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(assistant_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<Run>> {
    let row = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn list_for_conversation(pool: &PgPool, conversation_id: Uuid) -> AppResult<Vec<Run>> {
    let rows = sqlx::query_as::<_, Run>(
        "SELECT * FROM runs WHERE conversation_id = $1 ORDER BY started_at DESC",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
