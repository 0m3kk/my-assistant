pub mod artifact;
pub mod builtin;
pub mod file;
pub mod fs;
pub mod http_tool;
pub mod meta;
pub mod r#switch;
pub mod wasm;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::AppMode;
use crate::error::AppResult;
use crate::llm::{Provider, ToolDef};
use crate::secrets::SecretResolver;

/// What a tool announces to the LLM.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
}

impl From<&ToolDefinition> for ToolDef {
    fn from(d: &ToolDefinition) -> Self {
        ToolDef {
            name: d.name.clone(),
            description: d.description.clone(),
            parameters_schema: d.parameters_schema.clone(),
        }
    }
}

/// A resource a tool call intends to touch. The runtime checks these against
/// the tool's configured scope before execution.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceRef {
    /// "path" | "domain"
    pub kind: String,
    pub value: String,
}

impl ResourceRef {
    pub fn path(value: impl Into<String>) -> Self {
        Self {
            kind: "path".into(),
            value: value.into(),
        }
    }

    pub fn domain(value: impl Into<String>) -> Self {
        Self {
            kind: "domain".into(),
            value: value.into(),
        }
    }
}

/// Execution context handed to every tool. Carries everything a tool may need;
/// tools must not reach beyond it.
#[derive(Clone)]
pub struct ToolExecContext {
    pub user_id: Uuid,
    pub agent_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub cancel: CancellationToken,
    pub mode: AppMode,
    /// Per-conversation sandbox root (server mode / workspace tools).
    pub sandbox_root: PathBuf,
    /// Whitelisted absolute paths (local mode, from agent_tools config).
    pub allowed_paths: Vec<PathBuf>,
    /// Object storage for user-facing files (artifacts).
    pub storage: Arc<dyn crate::storage::Storage>,
    /// Size cap for files a tool may publish/read into storage.
    pub max_file_bytes: usize,
    pub secrets: Arc<SecretResolver>,
    pub http: reqwest::Client,
    pub pool: PgPool,
    /// Sandboxed Wasmtime runtime for `wasm` tools.
    pub wasm: Arc<wasm::WasmEngine>,
    /// Slug of the agent currently running — a switch tool must not target it.
    pub agent_slug: String,
    /// How many agent switches happened in this run (0 = the surface agent).
    pub switch_depth: u8,
    /// Provider registry, so a switch can resolve the new agent's model.
    pub providers: HashMap<String, Arc<dyn Provider>>,
    pub searxng_url: Option<String>,
    /// Defaults applied when building the next agent's tool registry.
    pub default_tool_timeout_ms: u64,
    pub default_tool_output_cap: usize,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    /// Resources this call will touch. Default: none.
    fn resources(&self, _input: &Value) -> Vec<ResourceRef> {
        Vec::new()
    }

    /// Slug of the agent this call hands control to, if the engine must switch
    /// agents instead of executing the tool. The engine handles those calls
    /// itself, so approvals and streaming work exactly as they do for the
    /// surface agent.
    fn switch_target(&self, _input: &Value) -> Option<String> {
        None
    }

    /// Whether a successful call ends the turn.
    ///
    /// Used by tools that hand something to a human — a change proposal must be
    /// reviewed before the model keeps working, so the run stops right after the
    /// tool produces its output instead of looping into another model call.
    fn ends_turn(&self) -> bool {
        false
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value>;
}

/// A tool as registered for one agent run — implementation + policy.
pub struct RegisteredTool {
    pub tool_id: Uuid,
    pub kind: String,
    pub config: Value,
    pub tool: Arc<dyn Tool>,
    pub requires_approval: bool,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

/// Dynamic registry merging builtins, DB-configured tools and per-agent
/// overrides for a single chat run.
pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

impl ToolRegistry {
    pub fn empty() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn insert(&mut self, registered: RegisteredTool) {
        self.tools
            .insert(registered.tool.definition().name, registered);
    }

    pub fn get(&self, name: &str) -> Option<&RegisteredTool> {
        self.tools.get(name)
    }

    #[allow(dead_code)]
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.tools.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    pub fn definitions(&self) -> Vec<ToolDef> {
        let mut defs: Vec<ToolDef> = self
            .tools
            .values()
            .map(|t| (&t.tool.definition()).into())
            .collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }
}

/// Tool failures that are safe to show to the LLM (and the user).
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Message(String),
    /// The call tried to reach a path outside the tool's configured scope.
    /// The engine turns this into an approval request.
    #[error("out_of_root:{0}")]
    OutOfRoot(String),
}

enum ScopeCheck {
    Ok,
    Denied(String),
    OutOfRoot(String),
}

/// Executes a tool with the shared policy: scope → timeout → output cap →
/// execution ledger → audit.
pub struct ToolRuntime;

impl ToolRuntime {
    pub async fn execute(
        registry: &ToolRegistry,
        ctx: &ToolExecContext,
        tool_call_id: &str,
        name: &str,
        input: Value,
    ) -> Result<Value, ToolError> {
        let Some(registered) = registry.get(name) else {
            return Err(ToolError::Message(format!("unknown tool: {name}")));
        };

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Message("run cancelled".into()));
        }
        // HTTP tool adapters use the shared client (connection pool).
        let _ = &ctx.http;

        // An approved approval row lets this specific call run even when it is
        // outside the configured root (the user chose "allow once").
        let scope_approved = crate::repo::approvals::by_conversation_and_call(
            &ctx.pool,
            ctx.conversation_id,
            tool_call_id,
        )
        .await
        .ok()
        .flatten()
        .is_some_and(|a| a.status == "approved");

        let resources = registered.tool.resources(&input);
        if !scope_approved {
            match check_scope(registered, ctx, &resources) {
                ScopeCheck::Ok => {}
                ScopeCheck::Denied(msg) => return Err(ToolError::Message(msg)),
                ScopeCheck::OutOfRoot(path) => return Err(ToolError::OutOfRoot(path)),
            }
        }

        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            registered.timeout,
            registered.tool.execute(ctx, input.clone()),
        )
        .await;

        let (status, output, error_text) = match result {
            Err(_) => (
                "timeout",
                None,
                Some(format!(
                    "tool timed out after {}ms",
                    registered.timeout.as_millis()
                )),
            ),
            Ok(Err(e)) => ("error", None, Some(sanitize_error(&e.to_string()))),
            Ok(Ok(value)) => (
                "ok",
                Some(cap_output(value, registered.max_output_bytes)),
                None,
            ),
        };
        let duration_ms = start.elapsed().as_millis().min(i32::MAX as u128) as i32;
        let resources_json = serde_json::to_value(&resources).ok();

        crate::repo::tool_executions::insert(
            &ctx.pool,
            Some(ctx.run_id),
            ctx.conversation_id,
            tool_call_id,
            Some(registered.tool_id),
            name,
            input,
            output.clone(),
            status,
            error_text.as_deref(),
            Some(duration_ms),
            resources_json,
        )
        .await
        .ok(); // ledger failure must not break the run

        crate::repo::audit::insert(
            &ctx.pool,
            Some(ctx.user_id),
            "tool.execute",
            &format!("{name} ({duration_ms}ms)"),
            status == "ok",
            serde_json::json!({
                "conversation_id": ctx.conversation_id,
                "agent_id": ctx.agent_id,
                "run_id": ctx.run_id,
                "status": status,
            }),
        )
        .await
        .ok();

        match status {
            "ok" => Ok(output.unwrap_or(Value::Null)),
            _ => Err(ToolError::Message(
                error_text.unwrap_or_else(|| "tool failed".into()),
            )),
        }
    }
}

fn check_scope(
    registered: &RegisteredTool,
    ctx: &ToolExecContext,
    resources: &[ResourceRef],
) -> ScopeCheck {
    if resources.is_empty() {
        return ScopeCheck::Ok;
    }
    match registered.kind.as_str() {
        "file" => check_file_scope(registered, ctx, resources),
        "http" => check_domain_scope(registered, resources),
        _ => ScopeCheck::Ok,
    }
}

fn check_file_scope(
    registered: &RegisteredTool,
    ctx: &ToolExecContext,
    resources: &[ResourceRef],
) -> ScopeCheck {
    let Some(root_raw) = registered.config.get("root").and_then(Value::as_str) else {
        return ScopeCheck::Denied("file tool has no configured root".into());
    };
    let root = if root_raw == "@workspace" {
        ctx.sandbox_root.clone()
    } else {
        PathBuf::from(root_raw)
    };
    let root_canonical = fs::canonical_deepest(&root).unwrap_or(root.clone());

    let allow = build_globs(registered.config.get("allow_globs"));
    let deny = build_globs(registered.config.get("deny_globs"));

    for r in resources.iter().filter(|r| r.kind == "path") {
        let path = std::path::Path::new(&r.value);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return ScopeCheck::OutOfRoot(r.value.clone());
        }
        let joined = root.join(path);
        let canonical = fs::canonical_deepest(&joined).unwrap_or(joined);
        if !canonical.starts_with(&root_canonical) {
            return ScopeCheck::OutOfRoot(r.value.clone());
        }
        let rel = canonical
            .strip_prefix(&root_canonical)
            .unwrap_or(&canonical)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(deny) = &deny {
            if deny.is_match(&rel) {
                return ScopeCheck::Denied(format!("path is denied by policy: {}", r.value));
            }
        }
        if let Some(allow) = &allow {
            if !allow.is_match(&rel) {
                return ScopeCheck::OutOfRoot(r.value.clone());
            }
        }
    }
    ScopeCheck::Ok
}

fn check_domain_scope(registered: &RegisteredTool, resources: &[ResourceRef]) -> ScopeCheck {
    let Some(base) = registered.config.get("base_url").and_then(Value::as_str) else {
        return ScopeCheck::Denied("http tool has no configured base_url".into());
    };
    let Ok(base_url) = reqwest::Url::parse(base) else {
        return ScopeCheck::Denied("http tool base_url is invalid".into());
    };
    let Some(base_host) = base_url.host_str() else {
        return ScopeCheck::Denied("http tool base_url has no host".into());
    };
    for r in resources.iter().filter(|r| r.kind == "domain") {
        if !r.value.eq_ignore_ascii_case(base_host) {
            return ScopeCheck::Denied(format!("domain not allowed: {}", r.value));
        }
    }
    ScopeCheck::Ok
}

fn build_globs(value: Option<&Value>) -> Option<globset::GlobSet> {
    let arr = value?.as_array()?;
    let mut builder = globset::GlobSetBuilder::new();
    let mut any = false;
    for item in arr {
        if let Some(pattern) = item.as_str() {
            if let Ok(glob) = globset::Glob::new(pattern) {
                builder.add(glob);
                any = true;
            }
        }
    }
    if any { builder.build().ok() } else { None }
}

/// Defensive error cleanup: strip anything that smells like a secret value and
/// cap the length so upstream error bodies cannot flood the context.
fn sanitize_error(msg: &str) -> String {
    let mut out: String = msg.chars().take(500).collect();
    out = out.replace("{{secret:", "«secret-ref:");
    out
}

fn cap_output(value: Value, max_output_bytes: usize) -> Value {
    let serialized = value.to_string();
    if serialized.len() <= max_output_bytes {
        return value;
    }
    let mut prefix: String = serialized.chars().take(max_output_bytes).collect();
    // Avoid splitting a UTF-8 char mid-boundary for the string consumer.
    while prefix.chars().last().is_some_and(|c| c.len_utf8() > 1) {
        prefix.pop();
    }
    serde_json::json!({
        "truncated": true,
        "note": "output truncated to fit max_output_bytes",
        "partial": prefix,
    })
}

/// Merge tool-level config with per-agent overrides (shallow, agent wins).
pub fn merge_config(base: &Value, override_: Option<&Value>) -> Value {
    let mut merged = base.as_object().cloned().unwrap_or_default();
    if let Some(extra) = override_.and_then(Value::as_object) {
        for (k, v) in extra {
            merged.insert(k.clone(), v.clone());
        }
    }
    Value::Object(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_merge_agent_wins() {
        let base = serde_json::json!({"timeout_ms": 1000, "a": 1});
        let over = serde_json::json!({"a": 2, "allowed_paths": ["/tmp/x"]});
        let merged = merge_config(&base, Some(&over));
        assert_eq!(merged["timeout_ms"], 1000);
        assert_eq!(merged["a"], 2);
        assert_eq!(merged["allowed_paths"][0], "/tmp/x");
    }

    #[test]
    fn output_capping() {
        let big = serde_json::json!({"s": "x".repeat(200)});
        let capped = cap_output(big, 50);
        assert_eq!(capped["truncated"], serde_json::json!(true));
        assert!(capped["partial"].as_str().unwrap().len() <= 50);
    }
}
