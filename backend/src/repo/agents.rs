use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{Agent, AgentToolRow};
use crate::error::{AppError, AppResult};

pub async fn list(pool: &PgPool) -> AppResult<Vec<Agent>> {
    // `general` first: it is the product's default agent.
    let rows = sqlx::query_as::<_, Agent>(
        "SELECT * FROM agents ORDER BY (slug = 'general') DESC, name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<Agent>> {
    let row = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn by_slug(pool: &PgPool, slug: &str) -> AppResult<Option<Agent>> {
    let row = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &PgPool,
    name: &str,
    slug: &str,
    description: &str,
    system_prompt: &str,
    default_model_id: Option<Uuid>,
    temperature: Option<f32>,
    max_tokens: Option<i32>,
    max_tool_calls: i32,
    created_by: Option<Uuid>,
) -> AppResult<Agent> {
    let row = sqlx::query_as::<_, Agent>(
        r#"INSERT INTO agents
           (name, slug, description, system_prompt, default_model_id, temperature, max_tokens, max_tool_calls, created_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING *"#,
    )
    .bind(name)
    .bind(slug)
    .bind(description)
    .bind(system_prompt)
    .bind(default_model_id)
    .bind(temperature)
    .bind(max_tokens)
    .bind(max_tool_calls)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            AppError::conflict("agent slug already exists")
        }
        other => other.into(),
    })?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    system_prompt: Option<&str>,
    default_model_id: Option<Option<Uuid>>,
    temperature: Option<Option<f32>>,
    max_tokens: Option<Option<i32>>,
    max_tool_calls: Option<i32>,
    status: Option<&str>,
    group_id: Option<Option<Uuid>>,
) -> AppResult<Agent> {
    let existing = by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("agent not found"))?;
    let row = sqlx::query_as::<_, Agent>(
        r#"UPDATE agents SET
            name = $2, description = $3, system_prompt = $4,
            default_model_id = COALESCE($5, default_model_id),
            temperature = COALESCE($6, temperature),
            max_tokens = COALESCE($7, max_tokens),
            max_tool_calls = COALESCE($8, max_tool_calls),
            status = $9,
            group_id = CASE WHEN $10::boolean THEN $11::uuid ELSE group_id END,
            revision = revision + 1,
            updated_at = now()
         WHERE id = $1 RETURNING *"#,
    )
    .bind(id)
    .bind(name.unwrap_or(&existing.name))
    .bind(description.unwrap_or(&existing.description))
    .bind(system_prompt.unwrap_or(&existing.system_prompt))
    .bind(default_model_id)
    .bind(temperature)
    .bind(max_tokens)
    .bind(max_tool_calls)
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
        .ok_or_else(|| AppError::not_found("agent not found"))?;
    if existing.is_builtin {
        return Err(AppError::conflict("built-in agents cannot be deleted"));
    }
    sqlx::query("DELETE FROM agents WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn snapshot(pool: &PgPool, id: Uuid) -> AppResult<serde_json::Value> {
    let agent = by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("agent not found"))?;
    let tools = tools_for_agent(pool, id).await?;
    let assignments: Vec<serde_json::Value> = tools
        .into_iter()
        .map(|(a, t)| {
            serde_json::json!({
                "tool_id": t.id,
                "name": t.name,
                "requires_approval": a.requires_approval,
                "config": a.config,
            })
        })
        .collect();
    Ok(serde_json::json!({
        "agent": serde_json::to_value(&agent).unwrap_or_default(),
        "tools": assignments,
    }))
}

// ---------- Agent-tool assignments ----------

pub async fn tools_for_agent(
    pool: &PgPool,
    agent_id: Uuid,
) -> AppResult<Vec<(AgentToolRow, crate::domain::ToolRow)>> {
    let assignments = sqlx::query_as::<_, AgentToolRow>(
        "SELECT agent_id, tool_id, requires_approval, config FROM agent_tools WHERE agent_id = $1",
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(assignments.len());
    for a in assignments {
        let tool = crate::repo::tools::by_id(pool, a.tool_id)
            .await?
            .filter(|t| t.status == "enabled")
            .ok_or_else(|| AppError::internal("agent references a missing/disabled tool"))?;
        out.push((a, tool));
    }
    Ok(out)
}

pub async fn assign_tool(
    pool: &PgPool,
    agent_id: Uuid,
    tool_id: Uuid,
    requires_approval: Option<bool>,
    config: Option<serde_json::Value>,
) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO agent_tools (agent_id, tool_id, requires_approval, config)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (agent_id, tool_id)
         DO UPDATE SET requires_approval = $3, config = $4"#,
    )
    .bind(agent_id)
    .bind(tool_id)
    .bind(requires_approval)
    .bind(config)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn unassign_tool(pool: &PgPool, agent_id: Uuid, tool_id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM agent_tools WHERE agent_id = $1 AND tool_id = $2")
        .bind(agent_id)
        .bind(tool_id)
        .execute(pool)
        .await?;
    Ok(())
}
