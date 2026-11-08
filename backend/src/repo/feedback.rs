use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::FeedbackRow;
use crate::error::AppResult;

#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    pool: &PgPool,
    user_id: Uuid,
    scope: &str,
    conversation_id: Option<Uuid>,
    run_id: Option<Uuid>,
    tool_execution_id: Option<Uuid>,
    tool_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    rating: i16,
    comment: &str,
    subject_key: &str,
) -> AppResult<FeedbackRow> {
    let row = sqlx::query_as::<_, FeedbackRow>(
        r#"INSERT INTO feedback
           (user_id, scope, conversation_id, run_id, tool_execution_id, tool_id, agent_id, rating, comment, subject_key)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           ON CONFLICT (user_id, subject_key) DO UPDATE SET
             rating = EXCLUDED.rating,
             comment = EXCLUDED.comment,
             status = 'open',
             updated_at = now()
           RETURNING *"#,
    )
    .bind(user_id)
    .bind(scope)
    .bind(conversation_id)
    .bind(run_id)
    .bind(tool_execution_id)
    .bind(tool_id)
    .bind(agent_id)
    .bind(rating)
    .bind(comment)
    .bind(subject_key)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn list_for_conversation(
    pool: &PgPool,
    user_id: Uuid,
    conversation_id: Uuid,
) -> AppResult<Vec<FeedbackRow>> {
    let rows = sqlx::query_as::<_, FeedbackRow>(
        "SELECT * FROM feedback WHERE user_id = $1 AND conversation_id = $2 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_admin(
    pool: &PgPool,
    status: Option<&str>,
    tool_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    limit: i64,
) -> AppResult<Vec<FeedbackRow>> {
    let rows = sqlx::query_as::<_, FeedbackRow>(
        r#"SELECT * FROM feedback
           WHERE ($1::text IS NULL OR status = $1)
             AND ($2::uuid IS NULL OR tool_id = $2)
             AND ($3::uuid IS NULL OR agent_id = $3)
           ORDER BY created_at DESC
           LIMIT $4"#,
    )
    .bind(status)
    .bind(tool_id)
    .bind(agent_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<FeedbackRow>> {
    let row = sqlx::query_as::<_, FeedbackRow>("SELECT * FROM feedback WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn mark_addressed(pool: &PgPool, ids: &[Uuid], proposal_id: Uuid) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE feedback SET status = 'addressed', addressed_by_proposal_id = $2, updated_at = now() WHERE id = ANY($1)",
    )
    .bind(ids)
    .bind(proposal_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(pool: &PgPool, id: Uuid, status: &str) -> AppResult<FeedbackRow> {
    let row = sqlx::query_as::<_, FeedbackRow>(
        "UPDATE feedback SET status = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(status)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM feedback WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}
