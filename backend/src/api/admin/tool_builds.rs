use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::api::admin::AdminUser;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::state::AppState;
use crate::tools::Tool;
use crate::tools::wasm::WasmTool;

pub async fn list(
    Path(tool_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let rows = repo::tool_builds::list_for_tool(&state.pool, tool_id).await?;
    Ok(Json(serde_json::json!({"builds": rows})))
}

pub async fn get(
    Path((tool_id, revision)): Path<(Uuid, i32)>,
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let row = repo::tool_builds::by_tool_revision(&state.pool, tool_id, revision)
        .await?
        .ok_or_else(|| AppError::not_found("build not found"))?;
    Ok(Json(serde_json::to_value(&row)?))
}

/// Admin-triggered build. This is deliberately NOT a tool exposed to agents.
pub async fn start(
    Path(tool_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
) -> AppResult<Json<serde_json::Value>> {
    let build = crate::service::build::start(&state, tool_id, admin.id).await?;
    Ok(Json(serde_json::to_value(&build)?))
}

#[derive(Debug, Deserialize)]
pub struct TestBody {
    #[serde(default)]
    pub input: Value,
}

/// Admin-only smoke test of a built wasm artifact. Runs without secrets and
/// with filesystem writes disabled; it is not an agent tool.
pub async fn test(
    Path(tool_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Json(body): Json<TestBody>,
) -> AppResult<Json<Value>> {
    let tool = repo::tools::by_id(&state.pool, tool_id)
        .await?
        .ok_or_else(|| AppError::not_found("tool not found"))?;
    if tool.kind != "wasm" {
        return Err(AppError::validation("only wasm tools can be tested"));
    }
    let sandbox = state
        .cfg
        .workspace_root()
        .join("admin-tools")
        .join(tool_id.to_string());
    tokio::fs::create_dir_all(&sandbox)
        .await
        .map_err(|e| AppError::internal(format!("cannot create test sandbox: {e}")))?;
    let ctx = crate::tools::ToolExecContext {
        user_id: admin.id,
        agent_id: Uuid::nil(),
        conversation_id: Uuid::nil(),
        run_id: Uuid::nil(),
        cancel: CancellationToken::new(),
        mode: state.cfg.mode,
        sandbox_root: sandbox,
        allowed_paths: Vec::new(),
        storage: Arc::clone(&state.storage),
        max_file_bytes: state.cfg.max_upload_bytes,
        secrets: Arc::clone(&state.secrets),
        http: state.http.clone(),
        pool: state.pool.clone(),
        wasm: Arc::clone(&state.wasm),
        // Admin test runs are not part of a conversation and never switch agents.
        agent_slug: String::new(),
        switch_depth: 0,
        providers: state.providers.clone(),
        searxng_url: state.cfg.searxng_url.clone(),
        default_tool_timeout_ms: 30_000,
        default_tool_output_cap: 64 * 1024,
    };
    let mut config = tool.config.clone();
    // Test runs never resolve secrets or write files.
    if let Some(caps) = config
        .get_mut("capabilities")
        .and_then(Value::as_object_mut)
    {
        caps.remove("secrets");
        if let Some(fs) = caps.get_mut("fs").and_then(Value::as_object_mut) {
            fs.insert("write".into(), json!(false));
        }
    }
    let wasm_tool = WasmTool {
        name: tool.name.clone(),
        description: tool.description.clone(),
        parameters_schema: tool.parameters_schema.clone(),
        config,
    };
    match wasm_tool.execute(&ctx, body.input).await {
        Ok(output) => Ok(Json(json!({"ok": true, "output": output}))),
        Err(e) => Ok(Json(json!({"ok": false, "error": e.detail}))),
    }
}
