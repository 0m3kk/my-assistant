//! Proposal workflow: meta-agents only propose; an admin applies or rejects.
//! Validation here is the single source of truth for tool/agent definitions.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::domain::{Agent, ChangeProposal, ToolRow};
use crate::error::{AppError, AppResult};
use crate::repo;

#[derive(Debug, Clone, Deserialize)]
pub struct ProposeRequest {
    /// "tool" | "agent"
    pub resource: String,
    /// "create" | "update"
    pub action: String,
    pub target_id: Option<Uuid>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub rationale: String,
    pub payload: Value,
    #[serde(default)]
    pub feedback_ids: Vec<Uuid>,
}

pub struct ValidatedTool {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub kind: String,
    pub config: Value,
    pub parameters_schema: Value,
    pub requires_approval: bool,
    /// Folder name to file the tool under; created on apply when missing.
    pub folder: Option<String>,
}

pub struct ValidatedAgent {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub system_prompt: String,
    pub default_model_id: Option<Uuid>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub max_tool_calls: i32,
    /// (name, id) pairs, resolved and checked (no meta tools).
    pub tools: Vec<(String, Uuid)>,
    /// Folder name to file the agent under; created on apply when missing.
    pub folder: Option<String>,
}

// ---------- Proposal creation ----------

pub async fn propose(
    pool: &PgPool,
    user_id: Uuid,
    agent_id: Uuid,
    conversation_id: Option<Uuid>,
    req: ProposeRequest,
) -> AppResult<ChangeProposal> {
    let action = match req.action.as_str() {
        "create" | "update" => req.action.as_str(),
        other => return Err(AppError::validation(format!("invalid action: {other}"))),
    };
    if action == "update" && req.target_id.is_none() {
        return Err(AppError::validation("target_id is required for update"));
    }
    if action == "create" && req.target_id.is_some() {
        return Err(AppError::validation("target_id must be absent for create"));
    }

    let (payload, before) = match req.resource.as_str() {
        "tool" => {
            check_secret_refs(pool, &req.payload).await?;
            let v = validate_tool(&req.payload)?;
            let payload = tool_payload_json(&v);
            let before = match (action, req.target_id) {
                ("update", Some(id)) => Some(repo::tools::snapshot(pool, id).await?),
                _ => None,
            };
            (payload, before)
        }
        "agent" => {
            let v = validate_agent(pool, &req.payload).await?;
            let payload = agent_payload_json(&v);
            let before = match (action, req.target_id) {
                ("update", Some(id)) => Some(repo::agents::snapshot(pool, id).await?),
                _ => None,
            };
            (payload, before)
        }
        other => return Err(AppError::validation(format!("invalid resource: {other}"))),
    };

    let title = if req.title.trim().is_empty() {
        format!("{} {}", action, req.resource)
    } else {
        req.title.trim().chars().take(200).collect()
    };

    repo::proposals::create(
        pool,
        &req.resource,
        action,
        req.target_id,
        &title,
        req.rationale.trim(),
        payload,
        before,
        &req.feedback_ids,
        Some(user_id),
        Some(agent_id),
        conversation_id,
    )
    .await
}

// ---------- Apply / reject ----------

pub async fn apply(
    pool: &PgPool,
    proposal_id: Uuid,
    admin_id: Uuid,
    edited_payload: Option<Value>,
    force: bool,
) -> AppResult<ChangeProposal> {
    let proposal = repo::proposals::by_id(pool, proposal_id)
        .await?
        .ok_or_else(|| AppError::not_found("proposal not found"))?;
    if proposal.status != "pending" {
        return Err(AppError::conflict("proposal is not pending"));
    }
    let payload = edited_payload.unwrap_or_else(|| proposal.payload.clone());

    let result = match proposal.resource.as_str() {
        "tool" => apply_tool(pool, &proposal, &payload, admin_id, force).await,
        "agent" => apply_agent(pool, &proposal, &payload, admin_id, force).await,
        other => Err(AppError::validation(format!("invalid resource: {other}"))),
    };

    match result {
        Ok(()) => {
            let updated = repo::proposals::mark_applied(pool, proposal_id, admin_id).await?;
            repo::audit::insert(
                pool,
                Some(admin_id),
                "proposal.apply",
                &format!("{}:{}", proposal.resource, proposal.title),
                true,
                json!({"proposal_id": proposal_id, "resource": proposal.resource, "action": proposal.action}),
            )
            .await
            .ok();
            Ok(updated)
        }
        Err(e) => {
            repo::proposals::set_error(pool, proposal_id, &e.detail)
                .await
                .ok();
            Err(e)
        }
    }
}

pub async fn reject(pool: &PgPool, proposal_id: Uuid, admin_id: Uuid) -> AppResult<ChangeProposal> {
    let proposal = repo::proposals::by_id(pool, proposal_id)
        .await?
        .ok_or_else(|| AppError::not_found("proposal not found"))?;
    if proposal.status != "pending" {
        return Err(AppError::conflict("proposal is not pending"));
    }
    let updated = repo::proposals::mark_rejected(pool, proposal_id, admin_id).await?;
    repo::audit::insert(
        pool,
        Some(admin_id),
        "proposal.reject",
        &format!("{}:{}", proposal.resource, proposal.title),
        true,
        json!({"proposal_id": proposal_id}),
    )
    .await
    .ok();
    Ok(updated)
}

async fn apply_tool(
    pool: &PgPool,
    proposal: &ChangeProposal,
    payload: &Value,
    admin_id: Uuid,
    force: bool,
) -> AppResult<()> {
    check_secret_refs(pool, payload).await?;
    let v = validate_tool(payload)?;

    if proposal.action == "update" {
        let target_id = proposal
            .target_id
            .ok_or_else(|| AppError::validation("target_id missing"))?;
        let current = repo::tools::snapshot(pool, target_id).await?;
        if let Some(before) = &proposal.before {
            if &current != before && !force {
                return Err(AppError::conflict(
                    "tool changed since the proposal was created; retry with force=true",
                ));
            }
        }
    }

    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    // File the tool under the folder it belongs to (created on demand).
    let group_id = match v.folder.as_deref() {
        Some(folder) => Some(folder_id_or_create(&mut tx, folder, false).await?),
        None => None,
    };
    let tool: ToolRow = match proposal.action.as_str() {
        "create" => {
            let status = if v.kind == "wasm" {
                "disabled"
            } else {
                "enabled"
            };
            let row = sqlx::query_as::<_, ToolRow>(
                r#"INSERT INTO tools
                   (name, display_name, description, kind, source_ref, config, parameters_schema,
                    requires_approval, is_meta, is_builtin, revision, status, created_by, group_id)
                   VALUES ($1,$2,$3,$4,'',$5,$6,$7,FALSE,FALSE,1,$8,$9,$10)
                   RETURNING *"#,
            )
            .bind(&v.name)
            .bind(&v.display_name)
            .bind(&v.description)
            .bind(&v.kind)
            .bind(&v.config)
            .bind(&v.parameters_schema)
            .bind(v.requires_approval)
            .bind(status)
            .bind(admin_id)
            .bind(group_id)
            .fetch_one(&mut *tx)
            .await?;
            if v.kind == "wasm" {
                create_pending_build(&mut tx, &row, &v.config, admin_id).await?;
            }
            // Let the proposing meta-agent use the new tool on its next turn.
            if let Some(agent_id) = proposal.created_by_agent_id {
                sqlx::query(
                    "INSERT INTO agent_tools (agent_id, tool_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(agent_id)
                .bind(row.id)
                .execute(&mut *tx)
                .await?;
            }
            row
        }
        "update" => {
            let target_id = proposal
                .target_id
                .ok_or_else(|| AppError::validation("target_id missing"))?;
            let status = if v.kind == "wasm" {
                "disabled"
            } else {
                "enabled"
            };
            let row = sqlx::query_as::<_, ToolRow>(
                r#"UPDATE tools SET
                     display_name = $2, description = $3, kind = $4, config = $5,
                     parameters_schema = $6, requires_approval = $7, status = $8,
                     group_id = COALESCE($9, group_id),
                     revision = revision + 1, updated_at = now()
                   WHERE id = $1 RETURNING *"#,
            )
            .bind(target_id)
            .bind(&v.display_name)
            .bind(&v.description)
            .bind(&v.kind)
            .bind(&v.config)
            .bind(&v.parameters_schema)
            .bind(v.requires_approval)
            .bind(status)
            .bind(group_id)
            .fetch_one(&mut *tx)
            .await?;
            if v.kind == "wasm" {
                create_pending_build(&mut tx, &row, &v.config, admin_id).await?;
            }
            row
        }
        _ => return Err(AppError::validation("invalid action")),
    };

    mark_addressed_and_applied(&mut tx, proposal, admin_id).await?;
    tx.commit().await?;

    // Version rows are written after the commit so the new tool row is visible
    // to this connection (avoids an uncommitted-row FK violation).
    repo::versions::insert_tool_version(
        pool,
        tool.id,
        tool.revision,
        serde_json::to_value(&tool)?,
        Some(admin_id),
        Some(proposal.id),
        if proposal.action == "create" {
            "proposal create"
        } else {
            "proposal update"
        },
    )
    .await?;
    Ok(())
}

async fn apply_agent(
    pool: &PgPool,
    proposal: &ChangeProposal,
    payload: &Value,
    admin_id: Uuid,
    force: bool,
) -> AppResult<()> {
    let v = validate_agent(pool, payload).await?;

    if proposal.action == "update" {
        let target_id = proposal
            .target_id
            .ok_or_else(|| AppError::validation("target_id missing"))?;
        let current = repo::agents::snapshot(pool, target_id).await?;
        if let Some(before) = &proposal.before {
            if &current != before && !force {
                return Err(AppError::conflict(
                    "agent changed since the proposal was created; retry with force=true",
                ));
            }
        }
    }

    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    // File the agent under the folder it belongs to (created on demand).
    let group_id = match v.folder.as_deref() {
        Some(folder) => Some(folder_id_or_create(&mut tx, folder, true).await?),
        None => None,
    };
    let agent: Agent = match proposal.action.as_str() {
        "create" => {
            let row = sqlx::query_as::<_, Agent>(
                r#"INSERT INTO agents
                   (name, slug, description, system_prompt, default_model_id, temperature,
                    max_tokens, max_tool_calls, kind, is_builtin, revision, status, created_by,
                    group_id)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'chat',FALSE,1,'enabled',$9,$10)
                   RETURNING *"#,
            )
            .bind(&v.name)
            .bind(&v.slug)
            .bind(&v.description)
            .bind(&v.system_prompt)
            .bind(v.default_model_id)
            .bind(v.temperature)
            .bind(v.max_tokens)
            .bind(v.max_tool_calls)
            .bind(admin_id)
            .bind(group_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| match e {
                sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                    AppError::conflict("agent slug already exists")
                }
                other => other.into(),
            })?;
            replace_agent_tools(&mut tx, row.id, &v.tools).await?;
            row
        }
        "update" => {
            let target_id = proposal
                .target_id
                .ok_or_else(|| AppError::validation("target_id missing"))?;
            let row = sqlx::query_as::<_, Agent>(
                r#"UPDATE agents SET
                     name = $2, description = $3, system_prompt = $4,
                     default_model_id = $5, temperature = $6, max_tokens = $7,
                     max_tool_calls = $8, group_id = COALESCE($9, group_id),
                     revision = revision + 1, updated_at = now()
                   WHERE id = $1 RETURNING *"#,
            )
            .bind(target_id)
            .bind(&v.name)
            .bind(&v.description)
            .bind(&v.system_prompt)
            .bind(v.default_model_id)
            .bind(v.temperature)
            .bind(v.max_tokens)
            .bind(v.max_tool_calls)
            .bind(group_id)
            .fetch_one(&mut *tx)
            .await?;
            replace_agent_tools(&mut tx, row.id, &v.tools).await?;
            row
        }
        _ => return Err(AppError::validation("invalid action")),
    };

    mark_addressed_and_applied(&mut tx, proposal, admin_id).await?;
    tx.commit().await?;

    repo::versions::insert_agent_version(
        pool,
        agent.id,
        agent.revision,
        json!({"agent": agent, "tools": v.tools.iter().map(|(n, _)| n).collect::<Vec<_>>()}),
        Some(admin_id),
        Some(proposal.id),
        "proposal apply",
    )
    .await?;
    Ok(())
}

async fn create_pending_build(
    tx: &mut Transaction<'_, Postgres>,
    tool: &ToolRow,
    config: &Value,
    admin_id: Uuid,
) -> AppResult<()> {
    let source = config
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let language = config
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("rust");
    sqlx::query(
        r#"INSERT INTO tool_builds (tool_id, revision, language, source, runtime, created_by)
           VALUES ($1, $2, $3, $4, 'wasm', $5)
           ON CONFLICT (tool_id, revision) DO UPDATE SET
             language = EXCLUDED.language, source = EXCLUDED.source,
             status = 'pending', build_log = NULL, finished_at = NULL"#,
    )
    .bind(tool.id)
    .bind(tool.revision)
    .bind(language)
    .bind(source)
    .bind(admin_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn replace_agent_tools(
    tx: &mut Transaction<'_, Postgres>,
    agent_id: Uuid,
    tools: &[(String, Uuid)],
) -> AppResult<()> {
    sqlx::query("DELETE FROM agent_tools WHERE agent_id = $1")
        .bind(agent_id)
        .execute(&mut **tx)
        .await?;
    for (_, tool_id) in tools {
        sqlx::query(
            "INSERT INTO agent_tools (agent_id, tool_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(agent_id)
        .bind(tool_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn mark_addressed_and_applied(
    tx: &mut Transaction<'_, Postgres>,
    proposal: &ChangeProposal,
    admin_id: Uuid,
) -> AppResult<()> {
    if !proposal.feedback_ids.is_empty() {
        sqlx::query(
            "UPDATE feedback SET status = 'addressed', addressed_by_proposal_id = $2, updated_at = now() WHERE id = ANY($1)",
        )
        .bind(&proposal.feedback_ids)
        .bind(proposal.id)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "UPDATE change_proposals SET status = 'applied', reviewed_by = $2, reviewed_at = now(), applied_at = now(), apply_error = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(proposal.id)
    .bind(admin_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// ---------- Validation ----------

pub fn validate_tool(payload: &Value) -> AppResult<ValidatedTool> {
    let obj = payload
        .as_object()
        .ok_or_else(|| AppError::validation("tool payload must be an object"))?;
    let name = required_str(obj, "name")?;
    if !valid_tool_name(name) {
        return Err(AppError::validation(
            "tool name must be lowercase [a-z0-9_.-], max 64 chars",
        ));
    }
    let kind = required_str(obj, "kind")?;
    let config = obj
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::validation("config must be an object"))?;
    let parameters_schema = obj
        .get("parameters_schema")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    validate_schema(&parameters_schema)?;

    let (config, requires_approval) = match kind {
        "http" => (validate_http_config(config)?, false),
        "file" => validate_file_config(config)?,
        "wasm" => (validate_wasm_config(config)?, false),
        "native" | "mcp" => {
            return Err(AppError::validation(
                "native/mcp tools must be created directly by an admin",
            ));
        }
        other => {
            return Err(AppError::validation(format!(
                "unsupported tool kind: {other}"
            )));
        }
    };

    let requires_approval = obj
        .get("requires_approval")
        .and_then(Value::as_bool)
        .unwrap_or(requires_approval);

    Ok(ValidatedTool {
        name: name.to_string(),
        display_name: obj
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_string(),
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        kind: kind.to_string(),
        config,
        parameters_schema,
        requires_approval,
        folder: folder_name(obj.get("folder"))?,
    })
}

pub async fn validate_agent(pool: &PgPool, payload: &Value) -> AppResult<ValidatedAgent> {
    let obj = payload
        .as_object()
        .ok_or_else(|| AppError::validation("agent payload must be an object"))?;
    let name = required_str(obj, "name")?;
    if name.trim().is_empty() || name.chars().count() > 128 {
        return Err(AppError::validation("agent name must be 1..128 chars"));
    }
    let slug = required_str(obj, "slug")?;
    if slug.is_empty()
        || slug.len() > 64
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(AppError::validation(
            "slug must be lowercase alphanumeric with - or _",
        ));
    }
    let system_prompt = obj
        .get("system_prompt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if system_prompt.len() > 64 * 1024 {
        return Err(AppError::validation("system_prompt is too large"));
    }

    let default_model_id = match obj.get("default_model_id") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let id = v
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| AppError::validation("default_model_id must be a uuid"))?;
            let status: Option<String> =
                sqlx::query_scalar("SELECT status FROM models WHERE id = $1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await?;
            match status.as_deref() {
                Some("enabled") => Some(id),
                Some(_) => return Err(AppError::validation("model is disabled")),
                None => return Err(AppError::validation("model not found")),
            }
        }
    };

    let temperature = match obj.get("temperature") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let t = v
                .as_f64()
                .ok_or_else(|| AppError::validation("temperature must be a number"))?;
            if !(0.0..=2.0).contains(&t) {
                return Err(AppError::validation("temperature must be 0..2"));
            }
            Some(t as f32)
        }
    };
    let max_tokens = match obj.get("max_tokens") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let n = v
                .as_i64()
                .ok_or_else(|| AppError::validation("max_tokens must be an integer"))?;
            if !(1..=1_000_000).contains(&n) {
                return Err(AppError::validation("max_tokens must be 1..1_000_000"));
            }
            Some(n as i32)
        }
    };
    let max_tool_calls = obj
        .get("max_tool_calls")
        .and_then(Value::as_i64)
        .unwrap_or(200);
    if !(0..=200).contains(&max_tool_calls) {
        return Err(AppError::validation("max_tool_calls must be 0..200"));
    }

    let tool_names: Vec<String> = obj
        .get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if tool_names.len() > 64 {
        return Err(AppError::validation("too many tools"));
    }
    let mut seen = BTreeSet::new();
    let mut tools = Vec::with_capacity(tool_names.len());
    for tool_name in tool_names {
        if !seen.insert(tool_name.clone()) {
            return Err(AppError::validation(format!("duplicate tool: {tool_name}")));
        }
        let row: Option<(Uuid, bool, String)> =
            sqlx::query_as("SELECT id, is_meta, status FROM tools WHERE name = $1")
                .bind(&tool_name)
                .fetch_optional(pool)
                .await?;
        let Some((id, is_meta, status)) = row else {
            return Err(AppError::validation(format!("tool not found: {tool_name}")));
        };
        if is_meta {
            return Err(AppError::validation(
                "meta tools cannot be assigned to a normal agent",
            ));
        }
        if status != "enabled" {
            return Err(AppError::validation(format!(
                "tool is disabled: {tool_name}"
            )));
        }
        tools.push((tool_name, id));
    }

    Ok(ValidatedAgent {
        name: name.trim().to_string(),
        slug: slug.to_string(),
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        system_prompt,
        default_model_id,
        temperature,
        max_tokens,
        max_tool_calls: max_tool_calls as i32,
        tools,
        folder: folder_name(obj.get("folder"))?,
    })
}

/// Normalise a `folder` field from a proposal payload: trimmed, non-empty,
/// length-capped. `None` means "leave it where the admin UI puts it".
fn folder_name(value: Option<&Value>) -> AppResult<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let name = value
        .as_str()
        .ok_or_else(|| AppError::validation("folder must be a string"))?
        .trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().count() > 64 {
        return Err(AppError::validation("folder must be at most 64 chars"));
    }
    Ok(Some(name.to_string()))
}

/// Resolve a folder name to an id, creating the folder when it does not exist.
/// Runs inside the apply transaction so a failed apply leaves no stray folder.
async fn folder_id_or_create(
    tx: &mut Transaction<'_, Postgres>,
    folder: &str,
    agent_folder: bool,
) -> AppResult<Uuid> {
    let (find, insert) = if agent_folder {
        (
            "SELECT id FROM agent_groups WHERE lower(name) = lower($1)",
            "INSERT INTO agent_groups (name, position) \
             VALUES ($1, COALESCE((SELECT MAX(position) + 1 FROM agent_groups), 0)) \
             RETURNING id",
        )
    } else {
        (
            "SELECT id FROM tool_groups WHERE lower(name) = lower($1)",
            "INSERT INTO tool_groups (name, position) \
             VALUES ($1, COALESCE((SELECT MAX(position) + 1 FROM tool_groups), 0)) \
             RETURNING id",
        )
    };

    if let Some(id) = sqlx::query_scalar::<_, Uuid>(find)
        .bind(folder)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(id);
    }
    let inserted = sqlx::query_scalar::<_, Uuid>(insert)
        .bind(folder)
        .fetch_one(&mut **tx)
        .await;
    match inserted {
        Ok(id) => Ok(id),
        // A concurrent apply created it first — read the winner's row.
        Err(sqlx::Error::Database(ref db)) if db.is_unique_violation() => {
            sqlx::query_scalar::<_, Uuid>(find)
                .bind(folder)
                .fetch_one(&mut **tx)
                .await
                .map_err(Into::into)
        }
        Err(other) => Err(other.into()),
    }
}

fn tool_payload_json(v: &ValidatedTool) -> Value {
    json!({
        "name": v.name,
        "display_name": v.display_name,
        "description": v.description,
        "kind": v.kind,
        "config": v.config,
        "parameters_schema": v.parameters_schema,
        "requires_approval": v.requires_approval,
        "folder": v.folder,
    })
}

fn agent_payload_json(v: &ValidatedAgent) -> Value {
    json!({
        "name": v.name,
        "slug": v.slug,
        "description": v.description,
        "system_prompt": v.system_prompt,
        "default_model_id": v.default_model_id,
        "temperature": v.temperature,
        "max_tokens": v.max_tokens,
        "max_tool_calls": v.max_tool_calls,
        "tools": v.tools.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "folder": v.folder,
    })
}

fn validate_schema(schema: &Value) -> AppResult<()> {
    let obj = schema
        .as_object()
        .ok_or_else(|| AppError::validation("parameters_schema must be an object"))?;
    if obj.get("type").and_then(Value::as_str) != Some("object") {
        return Err(AppError::validation(
            "parameters_schema.type must be 'object'",
        ));
    }
    Ok(())
}

fn validate_http_config(config: &Map<String, Value>) -> AppResult<Value> {
    allowed_keys(
        config,
        &[
            "base_url",
            "method",
            "path_template",
            "query",
            "headers",
            "body_template",
            "response_map",
            "timeout_ms",
            "max_output_bytes",
        ],
    )?;
    let base_url = required_str(config, "base_url")?;
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| AppError::validation("base_url must be an absolute URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::validation("base_url must be http(s) with a host"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::validation(
            "base_url must not contain a query or fragment",
        ));
    }
    let method = config
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return Err(AppError::validation("invalid HTTP method"));
    }
    let path = required_str(config, "path_template")?;
    if !path.starts_with('/') || path.contains("://") || path.split('/').any(|s| s == "..") {
        return Err(AppError::validation(
            "path_template must be an absolute path without '..'",
        ));
    }
    if let Some(headers) = config.get("headers") {
        let h = headers
            .as_object()
            .ok_or_else(|| AppError::validation("headers must be an object"))?;
        for (k, v) in h {
            if !v.is_string() {
                return Err(AppError::validation(format!("header {k} must be a string")));
            }
        }
    }
    if let Some(q) = config.get("query") {
        if !q.is_object() {
            return Err(AppError::validation("query must be an object"));
        }
    }
    let timeout_ms = numeric(config, "timeout_ms", 20_000, 1_000, 120_000)?;
    let max_output_bytes = numeric(config, "max_output_bytes", 262_144, 1_024, 1_048_576)?;

    let mut out = config.clone();
    out.insert("method".into(), json!(method));
    out.insert("timeout_ms".into(), json!(timeout_ms));
    out.insert("max_output_bytes".into(), json!(max_output_bytes));
    Ok(Value::Object(out))
}

fn validate_file_config(config: &Map<String, Value>) -> AppResult<(Value, bool)> {
    allowed_keys(
        config,
        &[
            "operation",
            "root",
            "allow_globs",
            "deny_globs",
            "max_file_bytes",
        ],
    )?;
    let operation = required_str(config, "operation")?;
    if !matches!(operation, "read" | "list" | "search" | "write" | "edit") {
        return Err(AppError::validation("invalid file operation"));
    }
    let root = required_str(config, "root")?;
    if root != "@workspace" && !std::path::Path::new(root).is_absolute() {
        return Err(AppError::validation(
            "root must be '@workspace' or an absolute path",
        ));
    }
    if root.contains("..") {
        return Err(AppError::validation("root must not contain '..'"));
    }
    for key in ["allow_globs", "deny_globs"] {
        if let Some(v) = config.get(key) {
            let arr = v
                .as_array()
                .ok_or_else(|| AppError::validation(format!("{key} must be an array")))?;
            for item in arr {
                let g = item.as_str().ok_or_else(|| {
                    AppError::validation(format!("{key} entries must be strings"))
                })?;
                if g.starts_with('/') || g.contains("..") {
                    return Err(AppError::validation(format!(
                        "{key} entries must be relative and must not contain '..'"
                    )));
                }
            }
        }
    }
    let max_file_bytes = numeric(config, "max_file_bytes", 1_048_576, 1_024, 16_777_216)?;
    let requires_approval = matches!(operation, "write" | "edit");

    let mut out = config.clone();
    out.insert("max_file_bytes".into(), json!(max_file_bytes));
    Ok((Value::Object(out), requires_approval))
}

fn validate_wasm_config(config: &Map<String, Value>) -> AppResult<Value> {
    allowed_keys(
        config,
        &[
            "runtime",
            "language",
            "entry",
            "source",
            "capabilities",
            "timeout_ms",
            "memory_limit_mb",
        ],
    )?;
    let runtime = config
        .get("runtime")
        .and_then(Value::as_str)
        .unwrap_or("wasm");
    if runtime != "wasm" {
        return Err(AppError::validation("runtime must be 'wasm'"));
    }
    let language = config
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("rust");
    if !matches!(language, "rust" | "assemblyscript") {
        return Err(AppError::validation(
            "language must be 'rust' or 'assemblyscript'",
        ));
    }
    let source = required_str(config, "source")?;
    if source.trim().is_empty() {
        return Err(AppError::validation("source must not be empty"));
    }
    if source.len() > 256 * 1024 {
        return Err(AppError::validation("source is too large"));
    }
    if let Some(caps) = config.get("capabilities") {
        validate_capabilities(caps)?;
    }
    let timeout_ms = numeric(config, "timeout_ms", 10_000, 1_000, 120_000)?;
    let memory_limit_mb = numeric(config, "memory_limit_mb", 64, 8, 512)?;
    let mut out = config.clone();
    out.insert("runtime".into(), json!("wasm"));
    out.insert("language".into(), json!(language));
    out.insert("timeout_ms".into(), json!(timeout_ms));
    out.insert("memory_limit_mb".into(), json!(memory_limit_mb));
    Ok(Value::Object(out))
}

fn validate_capabilities(caps: &Value) -> AppResult<()> {
    let obj = caps
        .as_object()
        .ok_or_else(|| AppError::validation("capabilities must be an object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "http" | "fs" | "secrets") {
            return Err(AppError::validation(format!("unknown capability: {key}")));
        }
    }
    if let Some(http) = obj.get("http") {
        let h = http
            .as_object()
            .ok_or_else(|| AppError::validation("capabilities.http must be an object"))?;
        if let Some(hosts) = h.get("allow_hosts") {
            let arr = hosts.as_array().ok_or_else(|| {
                AppError::validation("capabilities.http.allow_hosts must be an array")
            })?;
            if arr.iter().any(|v| !v.is_string()) {
                return Err(AppError::validation(
                    "capabilities.http.allow_hosts entries must be strings",
                ));
            }
        }
    }
    if let Some(fs) = obj.get("fs") {
        let f = fs
            .as_object()
            .ok_or_else(|| AppError::validation("capabilities.fs must be an object"))?;
        if let Some(roots) = f.get("roots") {
            let arr = roots
                .as_array()
                .ok_or_else(|| AppError::validation("capabilities.fs.roots must be an array"))?;
            if arr.iter().any(|v| !v.is_string()) {
                return Err(AppError::validation(
                    "capabilities.fs.roots entries must be strings",
                ));
            }
        }
        if let Some(w) = f.get("write") {
            if !w.is_boolean() {
                return Err(AppError::validation(
                    "capabilities.fs.write must be a boolean",
                ));
            }
        }
    }
    if let Some(secrets) = obj.get("secrets") {
        let arr = secrets
            .as_array()
            .ok_or_else(|| AppError::validation("capabilities.secrets must be an array"))?;
        if arr.iter().any(|v| !v.is_string()) {
            return Err(AppError::validation(
                "capabilities.secrets entries must be strings",
            ));
        }
    }
    Ok(())
}

fn allowed_keys(config: &Map<String, Value>, allowed: &[&str]) -> AppResult<()> {
    for key in config.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(AppError::validation(format!("unknown config key: {key}")));
        }
    }
    Ok(())
}

fn required_str<'a>(obj: &'a Map<String, Value>, key: &str) -> AppResult<&'a str> {
    obj.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::validation(format!("{key} is required")))
}

fn numeric(
    obj: &Map<String, Value>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> AppResult<i64> {
    let n = match obj.get(key) {
        None | Some(Value::Null) => default,
        Some(v) => v
            .as_i64()
            .ok_or_else(|| AppError::validation(format!("{key} must be an integer")))?,
    };
    if n < min || n > max {
        return Err(AppError::validation(format!(
            "{key} must be between {min} and {max}"
        )));
    }
    Ok(n)
}

fn valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
}

// ---------- Secret references ----------

fn extract_secret_refs(value: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_secret_refs(value, &mut out);
    out
}

fn collect_secret_refs(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            let mut rest = s.as_str();
            while let Some(start) = rest.find("{{secret:") {
                let after = &rest[start + "{{secret:".len()..];
                let Some(end) = after.find("}}") else { break };
                let name = &after[..end];
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                {
                    out.insert(name.to_string());
                }
                rest = &after[end + 2..];
            }
        }
        Value::Array(a) => a.iter().for_each(|v| collect_secret_refs(v, out)),
        Value::Object(o) => o.values().for_each(|v| collect_secret_refs(v, out)),
        _ => {}
    }
}

async fn check_secret_refs(pool: &PgPool, payload: &Value) -> AppResult<()> {
    let refs = extract_secret_refs(payload);
    if refs.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = refs.into_iter().collect();
    let existing: Vec<String> = sqlx::query_scalar("SELECT name FROM secrets WHERE name = ANY($1)")
        .bind(&names)
        .fetch_all(pool)
        .await?;
    let missing: Vec<&String> = names.iter().filter(|n| !existing.contains(n)).collect();
    if !missing.is_empty() {
        return Err(AppError::validation(format!(
            "unknown secret reference(s): {}",
            missing
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(())
}
