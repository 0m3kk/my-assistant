//! Idempotent seed for built-in tools, the general agent, and the four
//! meta-agents. Runs after migrations on every server start.
//!
//! Built-in tool definitions are refreshed from code; agent prompts are only
//! seeded when absent so admin/meta-agent improvements are not overwritten.

use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
async fn ensure_tool(
    pool: &PgPool,
    name: &str,
    display_name: &str,
    description: &str,
    source_ref: &str,
    schema: Value,
    requires_approval: bool,
    is_meta: bool,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO tools
             (name, display_name, description, kind, source_ref, config, parameters_schema,
              requires_approval, is_meta, is_builtin)
           VALUES ($1, $2, $3, 'builtin', $4, '{}'::jsonb, $5, $6, $7, TRUE)
           ON CONFLICT (name) DO UPDATE SET
             display_name = EXCLUDED.display_name,
             description = EXCLUDED.description,
             source_ref = EXCLUDED.source_ref,
             parameters_schema = EXCLUDED.parameters_schema,
             is_meta = EXCLUDED.is_meta
           RETURNING id"#,
    )
    .bind(name)
    .bind(display_name)
    .bind(description)
    .bind(source_ref)
    .bind(schema)
    .bind(requires_approval)
    .bind(is_meta)
    .fetch_one(pool)
    .await
}

/// Fill in missing tunables on the builtin `web_search` tool without
/// overwriting values an admin already saved.
async fn ensure_web_search_config(pool: &PgPool) -> Result<(), sqlx::Error> {
    let Some(stored) =
        sqlx::query_scalar::<_, Value>("SELECT config FROM tools WHERE name = 'web_search'")
            .fetch_optional(pool)
            .await?
    else {
        return Ok(());
    };
    let mut config = stored.as_object().cloned().unwrap_or_default();
    let defaults = json!({
        "max_results": 5,
        "timeout_ms": 15000,
        "max_output_bytes": 65536,
    });
    let mut changed = false;
    for (key, value) in defaults.as_object().into_iter().flatten() {
        if !config.contains_key(key) {
            config.insert(key.clone(), value.clone());
            changed = true;
        }
    }
    if changed {
        sqlx::query("UPDATE tools SET config = $2 WHERE name = $1")
            .bind("web_search")
            .bind(Value::Object(config))
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn ensure_chat_agent(
    pool: &PgPool,
    slug: &str,
    name: &str,
    description: &str,
    system_prompt: &str,
) -> Result<Uuid, sqlx::Error> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await?
    {
        sqlx::query("UPDATE agents SET kind = 'chat', is_builtin = TRUE WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        return Ok(id);
    }
    sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO agents
             (name, slug, description, system_prompt, temperature, max_tool_calls, kind, is_builtin)
           VALUES ($1, $2, $3, $4, 0.7, 200, 'chat', TRUE)
           RETURNING id"#,
    )
    .bind(name)
    .bind(slug)
    .bind(description)
    .bind(system_prompt)
    .fetch_one(pool)
    .await
}

async fn ensure_meta_agent(
    pool: &PgPool,
    slug: &str,
    name: &str,
    description: &str,
    system_prompt: &str,
    max_tool_calls: i32,
) -> Result<Uuid, sqlx::Error> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await?
    {
        return Ok(id);
    }
    sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO agents
             (name, slug, description, system_prompt, temperature, max_tool_calls, kind, is_builtin)
           VALUES ($1, $2, $3, $4, 0.3, $5, 'meta', TRUE)
           RETURNING id"#,
    )
    .bind(name)
    .bind(slug)
    .bind(description)
    .bind(system_prompt)
    .bind(max_tool_calls)
    .fetch_one(pool)
    .await
}

async fn assign(pool: &PgPool, agent_id: Uuid, tool_name: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO agent_tools (agent_id, tool_id) SELECT $1, id FROM tools WHERE name = $2 ON CONFLICT DO NOTHING",
    )
    .bind(agent_id)
    .bind(tool_name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn ensure(pool: &PgPool) -> Result<(), sqlx::Error> {
    // A previous process may have died mid-run: its in-memory task/token are
    // gone, so those rows would otherwise show "running" forever.
    sqlx::query("UPDATE conversations SET status = 'idle' WHERE status = 'running'")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE runs SET status = 'failed', finished_at = now() WHERE status = 'running'")
        .execute(pool)
        .await?;

    // ---------- Builtin tools ----------

    ensure_tool(
        pool,
        "workspace.list",
        "List workspace",
        "Lists files and folders in the conversation workspace.",
        "workspace_list",
        json!({"type":"object","properties":{"path":{"type":"string"}}}),
        false,
        false,
    )
    .await?;
    ensure_tool(
        pool,
        "workspace.read",
        "Read workspace file",
        "Reads a UTF-8 text file from the conversation workspace.",
        "workspace_read",
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        false,
        false,
    )
    .await?;
    ensure_tool(
        pool,
        "workspace.write",
        "Write workspace file",
        "Creates or overwrites a text file in the conversation workspace.",
        "workspace_write",
        json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"append":{"type":"boolean"}},"required":["path","content"]}),
        true,
        false,
    )
    .await?;
    ensure_tool(
        pool,
        "current_time",
        "Current time",
        "Returns the current server time in UTC (RFC 3339).",
        "current_time",
        json!({"type":"object","properties":{}}),
        false,
        false,
    )
    .await?;
    ensure_tool(
        pool,
        "web_search",
        "Web search",
        "Searches the web through the configured SearXNG instance and returns ranked results \
         (title, url, snippet).",
        "web_search",
        json!({
            "type":"object",
            "properties":{
                "query":{"type":"string"},
                "max_results":{"type":"integer","minimum":1,"maximum":20}
            },
            "required":["query"]
        }),
        false,
        false,
    )
    .await?;
    ensure_web_search_config(pool).await?;
    ensure_tool(
        pool,
        "save_artifact",
        "Save artifact",
        "Saves a downloadable artifact from workspace content or inline text.",
        "save_artifact",
        json!({"type":"object","properties":{"path":{"type":"string"},"filename":{"type":"string"},"content":{"type":"string"}}}),
        false,
        false,
    )
    .await?;

    // ---------- Meta tools (only assigned to meta-agents) ----------

    ensure_tool(
        pool,
        "meta.list_tools",
        "List tools",
        "List tool summaries (id, name, kind, status, revision, description).",
        "meta.list_tools",
        json!({"type":"object","properties":{"query":{"type":"string"}}}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.get_tool",
        "Get tool",
        "Get one tool definition, including config, source, and capabilities.",
        "meta.get_tool",
        json!({"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"}}}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.list_agents",
        "List agents",
        "List agent summaries.",
        "meta.list_agents",
        json!({"type":"object","properties":{"query":{"type":"string"}}}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.get_agent",
        "Get agent",
        "Get one agent definition, prompt, and assigned tools.",
        "meta.get_agent",
        json!({"type":"object","properties":{"id":{"type":"string"},"slug":{"type":"string"}}}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.list_feedback",
        "List feedback",
        "List user feedback for a tool or agent.",
        "meta.list_feedback",
        json!({"type":"object","properties":{"tool_id":{"type":"string"},"agent_id":{"type":"string"},"status":{"type":"string"},"limit":{"type":"integer"}}}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.list_executions",
        "List executions",
        "List recent tool executions (status, resources, errors).",
        "meta.list_executions",
        json!({"type":"object","properties":{"tool_id":{"type":"string"},"limit":{"type":"integer"}}}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.validate_tool",
        "Validate tool",
        "Validate an http/file/wasm tool payload without saving it.",
        "meta.validate_tool",
        json!({"type":"object","properties":{"payload":{"type":"object"}},"required":["payload"]}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.test_tool",
        "Test tool",
        "Run a restricted test of an http/file tool (no secrets, no writes).",
        "meta.test_tool",
        json!({"type":"object","properties":{"kind":{"type":"string"},"config":{"type":"object"},"parameters_schema":{"type":"object"},"input":{"type":"object"}},"required":["kind","config","input"]}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.propose_tool",
        "Propose tool",
        "Create a pending proposal to create or update a tool. A user must review and apply it.",
        "meta.propose_tool",
        json!({"type":"object","properties":{"action":{"type":"string"},"target_id":{"type":"string"},"title":{"type":"string"},"rationale":{"type":"string"},"payload":{"type":"object"},"feedback_ids":{"type":"array","items":{"type":"string"}}},"required":["action","payload"]}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.propose_agent",
        "Propose agent",
        "Create a pending proposal to create or update an agent. A user must review and apply it.",
        "meta.propose_agent",
        json!({"type":"object","properties":{"action":{"type":"string"},"target_id":{"type":"string"},"title":{"type":"string"},"rationale":{"type":"string"},"payload":{"type":"object"},"feedback_ids":{"type":"array","items":{"type":"string"}}},"required":["action","payload"]}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.list_proposals",
        "List proposals",
        "List proposals created in this conversation.",
        "meta.list_proposals",
        json!({"type":"object","properties":{"status":{"type":"string"}}}),
        false,
        true,
    )
    .await?;
    ensure_tool(
        pool,
        "meta.list_folders",
        "List folders",
        "List the existing tool and agent folders.",
        "meta.list_folders",
        json!({"type":"object","properties":{}}),
        false,
        true,
    )
    .await?;

    // ---------- General agent ----------

    ensure_tool(
        pool,
        "switch_agent",
        "Switch agent",
        "Hand the conversation to a specialist agent; it answers, then control returns.",
        "switch_agent",
        json!({
            "type": "object",
            "properties": {
                "agent": {"type": "string", "description": "Slug of the agent to switch to"},
                "task": {"type": "string", "description": "Complete instructions for the specialist"}
            },
            "required": ["agent", "task"]
        }),
        false,
        false,
    )
    .await?;

    let general = ensure_chat_agent(
        pool,
        "general",
        "General",
        "The default assistant. Answers directly, and switches to a specialist agent when a task needs one.",
        "You are the user's general assistant and the single entry point for every request. \
         Answer ordinary questions directly, concisely and accurately. When a task needs more \
         than conversation, use your tools for live data, files and search. When it needs a \
         specialist, call switch_agent with that agent's slug and a complete task description: \
         the specialist takes over with its own tools — for example, designing a new tool for \
         this system is `tool-writer`'s job, not something to script yourself. Never invent tool \
         output, and never claim work you did not do.",
    )
    .await?;
    for tool in [
        "current_time",
        "workspace.list",
        "workspace.read",
        "workspace.write",
        "web_search",
        "save_artifact",
        "switch_agent",
    ] {
        assign(pool, general, tool).await?;
    }

    // ---------- Meta-agents ----------

    let meta_tools = [
        "meta.list_tools",
        "meta.get_tool",
        "meta.list_agents",
        "meta.get_agent",
        "meta.list_feedback",
        "meta.list_executions",
        "meta.validate_tool",
        "meta.test_tool",
        "meta.propose_tool",
        "meta.propose_agent",
        "meta.list_proposals",
        "meta.list_folders",
    ];

    let tool_writer = ensure_meta_agent(
        pool,
        "tool-writer",
        "Tool Writer",
        "Designs new, narrowly scoped tools.",
        "You design tools for this application. Rules:\n\
         - Every tool must be narrowly scoped to one task.\n\
         - Use kind=http for one endpoint (fixed base_url + path_template).\n\
         - Use kind=file for a scoped root, with allow_globs/deny_globs.\n\
         - Use kind=wasm only for code that http/file cannot express; capabilities are explicit.\n\
         - wasm source is Rust exporting alloc(len)->ptr and run(ptr,len)->u64 returning (ptr<<32)|len; host imports are module \"host\": http, fs_read, fs_write, fs_list, secret_get, log; return UTF-8 JSON. See docs/TOOL_ABI.md.\n\
         - Put the tool in the folder it belongs to: pass payload.folder with an existing folder name. If no folder fits, pick a short descriptive one — it is created automatically.\n\
         - Use meta.validate_tool before proposing. The admin builds and tests wasm tools.\n\
         - Then call meta.propose_tool. Never claim a tool is saved: the user must review and apply it.",
        10,
    )
    .await?;
    for tool in meta_tools {
        assign(pool, tool_writer, tool).await?;
    }
    assign(pool, tool_writer, "web_search").await?;

    let tool_improver = ensure_meta_agent(
        pool,
        "tool-improver",
        "Tool Improver",
        "Improves existing tools from user feedback and execution history.",
        "You improve existing tools. Rules:\n\
         - Read feedback with meta.list_feedback and failures with meta.list_executions.\n\
         - Inspect the current definition with meta.get_tool.\n\
         - Ask the user clarifying questions before proposing a change.\n\
         - Validate and test your proposal, then call meta.propose_tool with action=update.\n\
         - Include the feedback_ids that motivated the change.\n\
         - Never claim the change is applied: the user must review and apply it.",
        10,
    )
    .await?;
    for tool in meta_tools {
        assign(pool, tool_improver, tool).await?;
    }
    assign(pool, tool_improver, "web_search").await?;

    let agent_writer = ensure_meta_agent(
        pool,
        "agent-writer",
        "Agent Writer",
        "Designs new agents and their tool assignments.",
        "You design agents. Rules:\n\
         - An agent is a system prompt + a set of existing tools.\n\
         - Inspect available tools with meta.list_tools before assigning them.\n\
         - Never assign meta.* tools to a normal agent.\n\
         - Put the agent in the folder it belongs to: pass payload.folder with an existing folder name. If no folder fits, pick a short descriptive one — it is created automatically.\n\
         - Then call meta.propose_agent. The user must review and apply it.",
        8,
    )
    .await?;
    for tool in meta_tools {
        assign(pool, agent_writer, tool).await?;
    }

    let agent_improver = ensure_meta_agent(
        pool,
        "agent-improver",
        "Agent Improver",
        "Improves existing agents from user feedback.",
        "You improve existing agents. Rules:\n\
         - Read feedback with meta.list_feedback and inspect the agent with meta.get_agent.\n\
         - Ask the user clarifying questions; prefer the smallest change that addresses the feedback.\n\
         - Then call meta.propose_agent with action=update and the feedback_ids.\n\
         - Never claim the change is applied: the user must review and apply it.",
        8,
    )
    .await?;
    for tool in meta_tools {
        assign(pool, agent_improver, tool).await?;
    }

    Ok(())
}
