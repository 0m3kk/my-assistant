use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::ChangeProposal;
use crate::error::AppResult;

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &PgPool,
    resource: &str,
    action: &str,
    target_id: Option<Uuid>,
    title: &str,
    rationale: &str,
    payload: Value,
    before: Option<Value>,
    feedback_ids: &[Uuid],
    created_by_user_id: Option<Uuid>,
    created_by_agent_id: Option<Uuid>,
    conversation_id: Option<Uuid>,
) -> AppResult<ChangeProposal> {
    let row = sqlx::query_as::<_, ChangeProposal>(
        r#"INSERT INTO change_proposals
           (resource, action, target_id, title, rationale, payload, before, feedback_ids,
            created_by_user_id, created_by_agent_id, conversation_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
           RETURNING *"#,
    )
    .bind(resource)
    .bind(action)
    .bind(target_id)
    .bind(title)
    .bind(rationale)
    .bind(payload)
    .bind(before)
    .bind(feedback_ids)
    .bind(created_by_user_id)
    .bind(created_by_agent_id)
    .bind(conversation_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn list(
    pool: &PgPool,
    status: Option<&str>,
    resource: Option<&str>,
) -> AppResult<Vec<ChangeProposal>> {
    let rows = sqlx::query_as::<_, ChangeProposal>(
        r#"SELECT * FROM change_proposals
           WHERE ($1::text IS NULL OR status = $1)
             AND ($2::text IS NULL OR resource = $2)
           ORDER BY created_at DESC"#,
    )
    .bind(status)
    .bind(resource)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<ChangeProposal>> {
    let row = sqlx::query_as::<_, ChangeProposal>("SELECT * FROM change_proposals WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn update_pending(
    pool: &PgPool,
    id: Uuid,
    payload: Value,
    title: &str,
    rationale: &str,
) -> AppResult<ChangeProposal> {
    let row = sqlx::query_as::<_, ChangeProposal>(
        "UPDATE change_proposals SET payload = $2, title = $3, rationale = $4, updated_at = now() WHERE id = $1 AND status = 'pending' RETURNING *",
    )
    .bind(id)
    .bind(payload)
    .bind(title)
    .bind(rationale)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn mark_applied(pool: &PgPool, id: Uuid, reviewed_by: Uuid) -> AppResult<ChangeProposal> {
    let row = sqlx::query_as::<_, ChangeProposal>(
        "UPDATE change_proposals SET status = 'applied', reviewed_by = $2, reviewed_at = now(), applied_at = now(), apply_error = NULL, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(reviewed_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn mark_rejected(
    pool: &PgPool,
    id: Uuid,
    reviewed_by: Uuid,
) -> AppResult<ChangeProposal> {
    let row = sqlx::query_as::<_, ChangeProposal>(
        "UPDATE change_proposals SET status = 'rejected', reviewed_by = $2, reviewed_at = now(), updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(reviewed_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn set_error(pool: &PgPool, id: Uuid, error: &str) -> AppResult<()> {
    sqlx::query("UPDATE change_proposals SET apply_error = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}
