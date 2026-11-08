//! `file` tools: read / list / search / write / edit inside a configured root.
//!
//! The tool config carries the scope:
//! ```json
//! {
//!   "operation": "read" | "list" | "search" | "write" | "edit",
//!   "root": "@workspace" | "/absolute/path",
//!   "allow_globs": ["src/**"],
//!   "deny_globs": [".git/**", ".env*"],
//!   "max_file_bytes": 1048576
//! }
//! ```
//! Paths are relative to the root. Traversal, absolute paths, and symlink
//! escapes are rejected. The runtime additionally checks the path against the
//! tool's scope before execution (out-of-root becomes an approval request).

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ResourceRef, Tool, ToolDefinition, ToolExecContext};
use crate::config::AppMode;
use crate::error::{AppError, AppResult};
use crate::tools::fs::canonical_deepest;

pub struct FileTool {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
    pub config: Value,
}

impl FileTool {
    fn operation(&self) -> &str {
        self.config
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("read")
    }

    fn root(&self, ctx: &ToolExecContext) -> AppResult<PathBuf> {
        let raw = self
            .config
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::internal("file tool has no root"))?;
        if raw == "@workspace" {
            return Ok(ctx.sandbox_root.clone());
        }
        if ctx.mode == AppMode::Server {
            return Err(AppError::forbidden(
                "absolute roots are disabled in server mode; use '@workspace'",
            ));
        }
        if !Path::new(raw).is_absolute() || raw.contains("..") {
            return Err(AppError::internal(
                "file tool root must be an absolute path",
            ));
        }
        Ok(PathBuf::from(raw))
    }

    fn max_file_bytes(&self, ctx: &ToolExecContext) -> usize {
        self.config
            .get("max_file_bytes")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(ctx.max_file_bytes)
    }

    fn resolved(&self, ctx: &ToolExecContext, raw: &str) -> AppResult<PathBuf> {
        let root = self.root(ctx)?;
        let root_canonical = canonical_deepest(&root)
            .map_err(|e| AppError::bad_request(format!("root unavailable: {e}")))?;
        let rel = Path::new(raw);
        if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(AppError::forbidden(
                "path must be relative and must not contain '..'",
            ));
        }
        let joined = root_canonical.join(rel);
        let canonical = canonical_deepest(&joined)
            .map_err(|e| AppError::bad_request(format!("cannot resolve path: {e}")))?;
        if !canonical.starts_with(&root_canonical) {
            return Err(AppError::forbidden("path escapes the configured root"));
        }
        let rel_str = canonical
            .strip_prefix(&root_canonical)
            .unwrap_or(&canonical)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(deny) = globs(self.config.get("deny_globs")) {
            if deny.is_match(&rel_str) {
                return Err(AppError::forbidden(format!(
                    "path is denied by policy: {raw}"
                )));
            }
        }
        if let Some(allow) = globs(self.config.get("allow_globs")) {
            if !allow.is_match(&rel_str) {
                return Err(AppError::forbidden(format!(
                    "path is outside the allowed globs: {raw}"
                )));
            }
        }
        Ok(canonical)
    }

    fn input_path<'a>(&self, input: &'a Value) -> AppResult<&'a str> {
        input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'path'"))
    }
}

fn globs(value: Option<&Value>) -> Option<globset::GlobSet> {
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

#[async_trait]
impl Tool for FileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters_schema: self.parameters_schema.clone(),
        }
    }

    fn resources(&self, input: &Value) -> Vec<ResourceRef> {
        let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
        vec![ResourceRef::path(path)]
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        match self.operation() {
            "read" => self.read(ctx, &input).await,
            "list" => self.list(ctx, &input).await,
            "search" => self.search(ctx, &input).await,
            "write" => self.write(ctx, &input).await,
            "edit" => self.edit(ctx, &input).await,
            other => Err(AppError::internal(format!(
                "unknown file operation: {other}"
            ))),
        }
    }
}

impl FileTool {
    async fn read(&self, ctx: &ToolExecContext, input: &Value) -> AppResult<Value> {
        let raw = self.input_path(input)?;
        let path = self.resolved(ctx, raw)?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| AppError::bad_request(format!("cannot read file: {e}")))?;
        if bytes.len() > self.max_file_bytes(ctx) {
            return Err(AppError::payload_too_large(format!(
                "file is too large (max {} bytes)",
                self.max_file_bytes(ctx)
            )));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| AppError::bad_request("file is not valid UTF-8"))?;
        Ok(json!({"path": raw, "content": content}))
    }

    async fn list(&self, ctx: &ToolExecContext, input: &Value) -> AppResult<Value> {
        let raw = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let path = self.resolved(ctx, raw)?;
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
            a["type"]
                .as_str()
                .unwrap_or("")
                .cmp(b["type"].as_str().unwrap_or(""))
                .then(
                    a["name"]
                        .as_str()
                        .unwrap_or("")
                        .cmp(b["name"].as_str().unwrap_or("")),
                )
        });
        Ok(json!({"path": raw, "entries": entries}))
    }

    async fn search(&self, ctx: &ToolExecContext, input: &Value) -> AppResult<Value> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'query'"))?;
        let case_sensitive = input
            .get("case_sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_results = input
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 200) as usize;
        let start_raw = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let start = self.resolved(ctx, start_raw)?;
        let root = self.root(ctx)?;
        let root_canonical = canonical_deepest(&root)
            .map_err(|e| AppError::bad_request(format!("root unavailable: {e}")))?;
        let allow = globs(self.config.get("allow_globs"));
        let deny = globs(self.config.get("deny_globs"));
        let max_bytes = self.max_file_bytes(ctx);
        let needle = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };

        let mut results = Vec::new();
        let mut stack = vec![start];
        while let Some(dir) = stack.pop() {
            if results.len() >= max_results {
                break;
            }
            let mut rd = match tokio::fs::read_dir(&dir).await {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            while let Some(entry) = rd.next_entry().await.ok().flatten() {
                if results.len() >= max_results {
                    break;
                }
                let path = entry.path();
                let Ok(meta) = entry.metadata().await else {
                    continue;
                };
                if meta.is_dir() {
                    stack.push(path);
                    continue;
                }
                if meta.len() > max_bytes as u64 {
                    continue;
                }
                let rel = match path.strip_prefix(&root_canonical) {
                    Ok(p) => p.to_string_lossy().replace('\\', "/"),
                    Err(_) => continue,
                };
                if let Some(deny) = &deny {
                    if deny.is_match(&rel) {
                        continue;
                    }
                }
                if let Some(allow) = &allow {
                    if !allow.is_match(&rel) {
                        continue;
                    }
                }
                let Ok(bytes) = tokio::fs::read(&path).await else {
                    continue;
                };
                let Ok(text) = String::from_utf8(bytes) else {
                    continue;
                };
                for (i, line) in text.lines().enumerate() {
                    let hay = if case_sensitive {
                        line.to_string()
                    } else {
                        line.to_lowercase()
                    };
                    if hay.contains(&needle) {
                        results.push(json!({"path": rel, "line": i + 1, "text": line}));
                        if results.len() >= max_results {
                            break;
                        }
                    }
                }
            }
        }
        Ok(json!({"query": query, "results": results, "truncated": results.len() >= max_results}))
    }

    async fn write(&self, ctx: &ToolExecContext, input: &Value) -> AppResult<Value> {
        let raw = self.input_path(input)?;
        let content = input
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'content'"))?;
        if content.len() > self.max_file_bytes(ctx) {
            return Err(AppError::payload_too_large(format!(
                "content is too large (max {} bytes)",
                self.max_file_bytes(ctx)
            )));
        }
        let path = self.resolved(ctx, raw)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AppError::bad_request(format!("cannot create parent directory: {e}"))
            })?;
        }
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| AppError::bad_request(format!("write failed: {e}")))?;
        Ok(json!({"path": raw, "bytes_written": content.len()}))
    }

    async fn edit(&self, ctx: &ToolExecContext, input: &Value) -> AppResult<Value> {
        let raw = self.input_path(input)?;
        let old = input
            .get("old_string")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'old_string'"))?;
        let new = input
            .get("new_string")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::bad_request("missing 'new_string'"))?;
        let replace_all = input
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if old.is_empty() {
            return Err(AppError::bad_request("old_string must not be empty"));
        }
        let path = self.resolved(ctx, raw)?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| AppError::bad_request(format!("cannot read file: {e}")))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::bad_request("file is not valid UTF-8"))?;
        let count = if replace_all {
            text.matches(old).count()
        } else {
            usize::from(text.contains(old))
        };
        if count == 0 {
            return Err(AppError::bad_request(
                "old_string was not found in the file",
            ));
        }
        let updated = if replace_all {
            text.replace(old, new)
        } else {
            text.replacen(old, new, 1)
        };
        tokio::fs::write(&path, updated)
            .await
            .map_err(|e| AppError::bad_request(format!("write failed: {e}")))?;
        Ok(json!({"path": raw, "replacements": count}))
    }
}
