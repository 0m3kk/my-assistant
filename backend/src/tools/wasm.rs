//! `wasm` tools: compiled artifacts (source + WASM binary).
//!
//! The build pipeline stores the artifact in object storage and records it in
//! `tool_builds`. Execution uses a sandboxed Wasmtime runtime with explicit
//! capability host functions; the source and capabilities were reviewed by an
//! admin before the build.

pub mod engine;
pub mod host;

pub use engine::WasmEngine;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ResourceRef, Tool, ToolDefinition, ToolExecContext};
use crate::error::{AppError, AppResult};

pub struct WasmTool {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
    pub config: Value,
}

#[async_trait]
impl Tool for WasmTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters_schema: self.parameters_schema.clone(),
        }
    }

    fn resources(&self, _input: &Value) -> Vec<ResourceRef> {
        Vec::new()
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        // Resolve the current successful artifact by tool name (the registry may
        // have been built before a rebuild completed).
        let tool = crate::repo::tools::by_name(&ctx.pool, &self.name)
            .await?
            .ok_or_else(|| AppError::not_found("wasm tool not found"))?;
        let build = crate::repo::tool_builds::latest_built(&ctx.pool, tool.id)
            .await?
            .ok_or_else(|| {
                AppError::not_found("wasm tool has no successful build; build it from the admin UI")
            })?;
        let (Some(storage_key), Some(sha256)) = (build.storage_key, build.sha256) else {
            return Err(AppError::not_found("wasm build has no artifact"));
        };
        let mut config = self.config.clone();
        config["artifact_storage_key"] = json!(storage_key);
        config["artifact_sha256"] = json!(sha256);
        ctx.wasm.execute(ctx, &config, input).await
    }
}
