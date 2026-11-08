//! Meta tools used by the four built-in meta-agents.
//!
//! These tools can read definitions, feedback, and executions, validate a
//! proposed definition, run a restricted test, and create a pending proposal.
//! They can never apply a proposal or build a wasm tool.

use async_trait::async_trait;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Tool, ToolDefinition, ToolExecContext};
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::service::proposals::{self, ProposeRequest};

fn def(name: &str, description: &str, schema: Value) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        parameters_schema: schema,
    }
}

fn opt_uuid(v: &Value, key: &str) -> AppResult<Option<Uuid>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Uuid::parse_str(s)
            .map(Some)
            .map_err(|_| AppError::bad_request(format!("{key} must be a uuid"))),
        _ => Err(AppError::bad_request(format!("{key} must be a uuid"))),
    }
}

fn tool_summary(t: &crate::domain::ToolRow) -> Value {
    json!({
        "id": t.id,
        "name": t.name,
        "display_name": t.display_name,
        "description": t.description,
        "kind": t.kind,
        "status": t.status,
        "revision": t.revision,
        "requires_approval": t.requires_approval,
    })
}

// ---------- meta.list_tools ----------

pub struct ListTools;

#[async_trait]
impl Tool for ListTools {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.list_tools",
            "List tools (id, name, kind, status, revision, description).",
            json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let rows = repo::tools::list_visible(&ctx.pool).await?;
        let q = input
            .get("query")
            .and_then(Value::as_str)
            .map(str::to_lowercase);
        let tools: Vec<Value> = rows
            .iter()
            .filter(|t| {
                q.as_deref().is_none_or(|q| {
                    t.name.to_lowercase().contains(q) || t.description.to_lowercase().contains(q)
                })
            })
            .map(tool_summary)
            .collect();
        Ok(json!({"tools": tools}))
    }
}

// ---------- meta.get_tool ----------

pub struct GetTool;

#[async_trait]
impl Tool for GetTool {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.get_tool",
            "Get one tool definition, including config, source, secret refs, and capabilities.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "name": {"type": "string"}
                }
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let tool = if let Some(id) = opt_uuid(&input, "id")? {
            repo::tools::by_id(&ctx.pool, id).await?
        } else if let Some(name) = input.get("name").and_then(Value::as_str) {
            repo::tools::by_name(&ctx.pool, name).await?
        } else {
            return Err(AppError::bad_request("provide id or name"));
        };
        let tool = tool.ok_or_else(|| AppError::not_found("tool not found"))?;
        Ok(json!({"tool": tool}))
    }
}

// ---------- meta.list_agents ----------

pub struct ListAgents;

#[async_trait]
impl Tool for ListAgents {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.list_agents",
            "List agents (id, name, slug, kind, status, revision, description).",
            json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let rows = repo::agents::list(&ctx.pool).await?;
        let q = input
            .get("query")
            .and_then(Value::as_str)
            .map(str::to_lowercase);
        let agents: Vec<Value> = rows
            .iter()
            .filter(|a| {
                q.as_deref().is_none_or(|q| {
                    a.name.to_lowercase().contains(q)
                        || a.slug.to_lowercase().contains(q)
                        || a.description.to_lowercase().contains(q)
                })
            })
            .map(|a| {
                json!({
                    "id": a.id,
                    "name": a.name,
                    "slug": a.slug,
                    "description": a.description,
                    "kind": a.kind,
                    "status": a.status,
                    "revision": a.revision,
                })
            })
            .collect();
        Ok(json!({"agents": agents}))
    }
}

// ---------- meta.get_agent ----------

pub struct GetAgent;

#[async_trait]
impl Tool for GetAgent {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.get_agent",
            "Get one agent definition, its system prompt, and assigned tools.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "slug": {"type": "string"}
                }
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let agent = if let Some(id) = opt_uuid(&input, "id")? {
            repo::agents::by_id(&ctx.pool, id).await?
        } else if let Some(slug) = input.get("slug").and_then(Value::as_str) {
            repo::agents::by_slug(&ctx.pool, slug).await?
        } else {
            return Err(AppError::bad_request("provide id or slug"));
        };
        let agent = agent.ok_or_else(|| AppError::not_found("agent not found"))?;
        let tools = repo::agents::tools_for_agent(&ctx.pool, agent.id).await?;
        let assignments: Vec<Value> = tools
            .into_iter()
            .map(|(a, t)| {
                json!({
                    "tool_id": t.id,
                    "name": t.name,
                    "kind": t.kind,
                    "requires_approval": a.requires_approval.unwrap_or(t.requires_approval),
                })
            })
            .collect();
        Ok(json!({"agent": agent, "tools": assignments}))
    }
}

// ---------- meta.list_feedback ----------

pub struct ListFeedback;

#[async_trait]
impl Tool for ListFeedback {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.list_feedback",
            "List user feedback for a tool or agent, optionally filtered by status.",
            json!({
                "type": "object",
                "properties": {
                    "tool_id": {"type": "string"},
                    "agent_id": {"type": "string"},
                    "status": {"type": "string", "enum": ["open", "addressed", "dismissed"]},
                    "limit": {"type": "integer"}
                }
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let tool_id = opt_uuid(&input, "tool_id")?;
        let agent_id = opt_uuid(&input, "agent_id")?;
        let status = input.get("status").and_then(Value::as_str);
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 200) as i64;
        let rows = repo::feedback::list_admin(&ctx.pool, status, tool_id, agent_id, limit).await?;
        Ok(json!({"feedback": rows}))
    }
}

// ---------- meta.list_executions ----------

pub struct ListExecutions;

#[async_trait]
impl Tool for ListExecutions {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.list_executions",
            "List recent tool executions (status, resources, errors) as an improvement signal.",
            json!({
                "type": "object",
                "properties": {
                    "tool_id": {"type": "string"},
                    "limit": {"type": "integer"}
                }
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 100) as i64;
        let rows = if let Some(tool_id) = opt_uuid(&input, "tool_id")? {
            repo::tool_executions::list_for_tool(&ctx.pool, tool_id, limit).await?
        } else {
            repo::tool_executions::list_for_conversation(&ctx.pool, ctx.conversation_id, limit)
                .await?
        };
        Ok(json!({"executions": rows}))
    }
}

// ---------- meta.validate_tool ----------

pub struct ValidateTool;

#[async_trait]
impl Tool for ValidateTool {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.validate_tool",
            "Validate a tool payload (http/file/wasm) without saving it.",
            json!({
                "type": "object",
                "properties": {"payload": {"type": "object"}},
                "required": ["payload"]
            }),
        )
    }

    async fn execute(&self, _ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let payload = input
            .get("payload")
            .ok_or_else(|| AppError::bad_request("missing payload"))?;
        match proposals::validate_tool(payload) {
            Ok(_) => Ok(json!({"ok": true})),
            Err(e) => Ok(json!({"ok": false, "error": e.detail})),
        }
    }
}

// ---------- meta.test_tool ----------

pub struct TestTool;

#[async_trait]
impl Tool for TestTool {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.test_tool",
            "Run a restricted test of an http/file tool. Never resolves secrets and never performs writes.",
            json!({
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["http", "file"]},
                    "config": {"type": "object"},
                    "parameters_schema": {"type": "object"},
                    "input": {"type": "object"}
                },
                "required": ["kind", "config", "input"]
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let kind = input
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing kind"))?;
        let mut config = input
            .get("config")
            .cloned()
            .ok_or_else(|| AppError::bad_request("missing config"))?;
        let call_input = input.get("input").cloned().unwrap_or_else(|| json!({}));
        strip_secret_refs(&mut config);
        match kind {
            "http" => {
                let tool = super::http_tool::HttpTool {
                    name: "meta.test".into(),
                    description: String::new(),
                    parameters_schema: json!({"type": "object"}),
                    config,
                };
                match tool.execute(ctx, call_input).await {
                    Ok(v) => Ok(json!({"ok": true, "output": v})),
                    Err(e) => Ok(json!({"ok": false, "error": e.to_string()})),
                }
            }
            "file" => {
                let operation = config
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("read");
                if matches!(operation, "write" | "edit") {
                    return Ok(json!({
                        "ok": false,
                        "error": "write/edit operations are not executed by meta.test_tool"
                    }));
                }
                let tool = super::file::FileTool {
                    name: "meta.test".into(),
                    description: String::new(),
                    parameters_schema: json!({"type": "object"}),
                    config,
                };
                match tool.execute(ctx, call_input).await {
                    Ok(v) => Ok(json!({"ok": true, "output": v})),
                    Err(e) => Ok(json!({"ok": false, "error": e.to_string()})),
                }
            }
            other => Ok(json!({"ok": false, "error": format!("unsupported kind: {other}")})),
        }
    }
}

fn strip_secret_refs(value: &mut Value) {
    match value {
        Value::String(s) => {
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(start) = rest.find("{{secret:") {
                out.push_str(&rest[..start]);
                let after = &rest[start + "{{secret:".len()..];
                let Some(end) = after.find("}}") else {
                    out.push_str("{{secret:");
                    rest = after;
                    continue;
                };
                rest = &after[end + 2..];
            }
            out.push_str(rest);
            *s = out;
        }
        Value::Array(a) => a.iter_mut().for_each(strip_secret_refs),
        Value::Object(o) => o.values_mut().for_each(strip_secret_refs),
        _ => {}
    }
}

// ---------- meta.propose_tool / meta.propose_agent ----------

pub struct ProposeTool;

#[async_trait]
impl Tool for ProposeTool {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.propose_tool",
            "Create a pending proposal to create or update a tool. The user must review and apply it. \
             Set payload.folder to file the tool under a folder (created if it does not exist); call \
             meta.list_folders to see the existing ones.",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["create", "update"]},
                    "target_id": {"type": "string"},
                    "title": {"type": "string"},
                    "rationale": {"type": "string"},
                    "payload": {
                        "type": "object",
                        "properties": {
                            "folder": {
                                "type": "string",
                                "description": "Folder name; created when missing"
                            }
                        }
                    },
                    "feedback_ids": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["action", "payload"]
            }),
        )
    }

    /// The admin reviews the proposal before the conversation continues.
    fn ends_turn(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let req = ProposeRequest {
            resource: "tool".into(),
            action: input
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("create")
                .to_string(),
            target_id: opt_uuid(&input, "target_id")?,
            title: input
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            rationale: input
                .get("rationale")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            payload: input
                .get("payload")
                .cloned()
                .ok_or_else(|| AppError::bad_request("missing payload"))?,
            feedback_ids: uuid_list(&input, "feedback_ids")?,
        };
        let proposal = proposals::propose(
            &ctx.pool,
            ctx.user_id,
            ctx.agent_id,
            Some(ctx.conversation_id),
            req,
        )
        .await?;
        Ok(json!({
            "proposal_id": proposal.id,
            "status": proposal.status,
            "resource": proposal.resource,
            "action": proposal.action,
            "title": proposal.title
        }))
    }
}

pub struct ProposeAgent;

#[async_trait]
impl Tool for ProposeAgent {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.propose_agent",
            "Create a pending proposal to create or update an agent. The user must review and apply it. \
             Set payload.folder to file the agent under a folder (created if it does not exist); call \
             meta.list_folders to see the existing ones.",
            json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["create", "update"]},
                    "target_id": {"type": "string"},
                    "title": {"type": "string"},
                    "rationale": {"type": "string"},
                    "payload": {
                        "type": "object",
                        "properties": {
                            "folder": {
                                "type": "string",
                                "description": "Folder name; created when missing"
                            }
                        }
                    },
                    "feedback_ids": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["action", "payload"]
            }),
        )
    }

    /// The admin reviews the proposal before the conversation continues.
    fn ends_turn(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let req = ProposeRequest {
            resource: "agent".into(),
            action: input
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("create")
                .to_string(),
            target_id: opt_uuid(&input, "target_id")?,
            title: input
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            rationale: input
                .get("rationale")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            payload: input
                .get("payload")
                .cloned()
                .ok_or_else(|| AppError::bad_request("missing payload"))?,
            feedback_ids: uuid_list(&input, "feedback_ids")?,
        };
        let proposal = proposals::propose(
            &ctx.pool,
            ctx.user_id,
            ctx.agent_id,
            Some(ctx.conversation_id),
            req,
        )
        .await?;
        Ok(json!({
            "proposal_id": proposal.id,
            "status": proposal.status,
            "resource": proposal.resource,
            "action": proposal.action,
            "title": proposal.title
        }))
    }
}

// ---------- meta.list_proposals ----------

pub struct ListProposals;

#[async_trait]
impl Tool for ListProposals {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.list_proposals",
            "List proposals created in this conversation (status, resource, action, title).",
            json!({
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["pending", "applied", "rejected"]}
                }
            }),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let status = input.get("status").and_then(Value::as_str);
        let rows = repo::proposals::list(&ctx.pool, status, None).await?;
        let out: Vec<Value> = rows
            .into_iter()
            .filter(|p| p.conversation_id == Some(ctx.conversation_id))
            .map(|p| {
                json!({
                    "id": p.id,
                    "resource": p.resource,
                    "action": p.action,
                    "status": p.status,
                    "title": p.title,
                    "apply_error": p.apply_error,
                })
            })
            .collect();
        Ok(json!({"proposals": out}))
    }
}

// ---------- meta.list_folders ----------

pub struct ListFolders;

#[async_trait]
impl Tool for ListFolders {
    fn definition(&self) -> ToolDefinition {
        def(
            "meta.list_folders",
            "List the existing tool and agent folders, so a proposal can be filed \
             under an existing one instead of creating a near-duplicate.",
            json!({"type": "object", "properties": {}}),
        )
    }

    async fn execute(&self, ctx: &ToolExecContext, _input: Value) -> AppResult<Value> {
        let tool_folders = repo::tool_groups::list(&ctx.pool)
            .await?
            .into_iter()
            .map(|g| g.name)
            .collect::<Vec<_>>();
        let agent_folders = repo::agent_groups::list(&ctx.pool)
            .await?
            .into_iter()
            .map(|g| g.name)
            .collect::<Vec<_>>();
        Ok(json!({
            "tool_folders": tool_folders,
            "agent_folders": agent_folders,
        }))
    }
}

fn uuid_list(input: &Value, key: &str) -> AppResult<Vec<Uuid>> {
    let Some(arr) = input.get(key).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    arr.iter()
        .map(|v| {
            v.as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| AppError::bad_request(format!("{key} entries must be uuids")))
        })
        .collect()
}
