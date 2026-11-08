//! Workspace tools: list / read / write inside the conversation sandbox.
//!
//! Paths are relative to the sandbox root. Traversal, absolute paths, and
//! symlink escapes are rejected by [`PathGuard`].

mod guard;

pub use guard::{PathGuard, canonical_deepest};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Tool, ToolDefinition, ToolExecContext};
use crate::error::{AppError, AppResult};

fn denied(e: guard::PathDenied) -> AppError {
    AppError::forbidden(e.to_string())
}

fn workspace_path(ctx: &ToolExecContext, raw: &str) -> AppResult<std::path::PathBuf> {
    let guard = guard::PathGuard::new(
        crate::config::AppMode::Server,
        ctx.sandbox_root.clone(),
        Vec::new(),
    );
    guard.resolve(raw, false).map_err(denied)
}

// ---------- workspace.list ----------

pub struct FsList;

#[async_trait]
impl Tool for FsList {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "workspace.list".into(),
            description: "Lists files and folders in the conversation workspace.".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Relative path (default '.')"}},
                "required": [],
            }),
        }
    }

    fn resources(&self, input: &Value) -> Vec<super::ResourceRef> {
        vec![super::ResourceRef::path(
            input
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or(".")
                .to_string(),
        )]
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let raw = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let path = workspace_path(ctx, raw)?;
        let mut entries = Vec::new();
        let mut rd = tokio::fs::read_dir(&path)
            .await
            .map_err(|e| AppError::bad_request(format!("cannot list directory: {e}")))?;
        while let Some(entry) = rd
            .next_entry()
            .await
            .map_err(|e| AppError::bad_request(format!("read_dir failed: {e}")))?
        {
            let ft = entry.file_type().await.ok();
            entries.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "type": if ft.is_some_and(|t| t.is_dir()) { "dir" } else { "file" },
            }));
        }
        entries.sort_by(|a, b| {
            let ka = a["type"].as_str().unwrap_or("");
            let kb = b["type"].as_str().unwrap_or("");
            ka.cmp(kb).then(
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or("")),
            )
        });
        Ok(json!({
            "path": path.to_string_lossy(),
            "entries": entries,
        }))
    }
}

// ---------- workspace.read ----------

pub struct FsRead;

#[async_trait]
impl Tool for FsRead {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "workspace.read".into(),
            description: "Reads a UTF-8 text file from the conversation workspace.".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Relative path"}},
                "required": ["path"],
            }),
        }
    }

    fn resources(&self, input: &Value) -> Vec<super::ResourceRef> {
        vec![super::ResourceRef::path(
            input
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )]
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let raw = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'path'"))?;
        let path = workspace_path(ctx, raw)?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| AppError::bad_request(format!("cannot read file: {e}")))?;
        if bytes.len() > ctx.max_file_bytes {
            return Err(AppError::payload_too_large(format!(
                "file is too large (max {} bytes)",
                ctx.max_file_bytes
            )));
        }
        let content = String::from_utf8(bytes).map_err(|_| {
            AppError::bad_request("file is not valid UTF-8 (binary files are not supported)")
        })?;
        Ok(json!({
            "path": path.to_string_lossy(),
            "content": content,
        }))
    }
}

// ---------- workspace.write ----------

pub struct FsWrite;

#[async_trait]
impl Tool for FsWrite {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "workspace.write".into(),
            description: "Creates or overwrites a text file in the conversation workspace.".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Relative path"},
                    "content": {"type": "string"},
                    "append": {"type": "boolean"},
                },
                "required": ["path", "content"],
            }),
        }
    }

    fn resources(&self, input: &Value) -> Vec<super::ResourceRef> {
        vec![super::ResourceRef::path(
            input
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )]
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let raw = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'path'"))?;
        let content = input
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'content'"))?;
        if content.len() > ctx.max_file_bytes {
            return Err(AppError::payload_too_large(format!(
                "content is too large (max {} bytes)",
                ctx.max_file_bytes
            )));
        }
        let append = input
            .get("append")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let guard = guard::PathGuard::new(
            crate::config::AppMode::Server,
            ctx.sandbox_root.clone(),
            Vec::new(),
        );
        let path = guard.resolve(raw, true).map_err(denied)?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AppError::bad_request(format!("cannot create parent directory: {e}"))
            })?;
        }
        if append {
            use tokio::io::AsyncWriteExt;
            let mut f = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .map_err(|e| AppError::bad_request(format!("cannot open file: {e}")))?;
            f.write_all(content.as_bytes())
                .await
                .map_err(|e| AppError::bad_request(format!("write failed: {e}")))?;
        } else {
            tokio::fs::write(&path, content)
                .await
                .map_err(|e| AppError::bad_request(format!("write failed: {e}")))?;
        }
        Ok(json!({
            "path": path.to_string_lossy(),
            "bytes_written": content.len(),
            "append": append,
        }))
    }
}
