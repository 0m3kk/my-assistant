//! `save_artifact` — publishes a file as a downloadable artifact.
//!
//! Two modes:
//! - `{path}`: publish an existing file from the permitted filesystem area
//!   (workspace sandbox / whitelisted dirs) — copied into object storage.
//! - `{filename, content}`: create a text artifact from inline content.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::{Value, json};

use super::fs::PathGuard;
use super::{Tool, ToolDefinition, ToolExecContext};
use crate::config::AppMode;
use crate::error::{AppError, AppResult};
use crate::service::files::{self, ARTIFACT_INLINE_CAP};

pub struct SaveArtifact;

/// Resolve a `save_artifact` path.
///
/// Conversation-workspace paths are always readable, in both `server` and
/// `local` mode — that is where `workspace.*` tools put files and where the
/// Workspace panel reads from, so publishing them must not depend on the
/// local-mode whitelist. Anything else falls back to the run's normal policy
/// (local-mode whitelisted roots).
fn resolve_artifact_path(
    mode: AppMode,
    sandbox_root: &Path,
    allowed_paths: &[PathBuf],
    raw: &str,
) -> AppResult<PathBuf> {
    // Accept `@workspace/...` as an explicit workspace reference too.
    let workspace_relative = raw
        .strip_prefix("@workspace/")
        .or_else(|| raw.strip_prefix("@workspace"))
        .unwrap_or(raw);
    let workspace_guard = PathGuard::new(AppMode::Server, sandbox_root.to_path_buf(), Vec::new());
    if let Ok(path) = workspace_guard.resolve(workspace_relative, false) {
        return Ok(path);
    }
    PathGuard::new(mode, sandbox_root.to_path_buf(), allowed_paths.to_vec())
        .resolve(raw, false)
        .map_err(|e| AppError::forbidden(format!("cannot access path for artifact: {e}")))
}

#[async_trait]
impl Tool for SaveArtifact {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "save_artifact".into(),
            description: "Saves a file the user can download. Either publish an existing \
                          workspace file via \"path\", or create a new text file from \
                          \"content\" (requires \"filename\")."
                .into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "filename": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": [],
            }),
        }
    }

    async fn execute(&self, ctx: &ToolExecContext, input: Value) -> AppResult<Value> {
        let (filename, content_type, data) = if let Some(raw_path) =
            input.get("path").and_then(Value::as_str)
        {
            let resolved =
                resolve_artifact_path(ctx.mode, &ctx.sandbox_root, &ctx.allowed_paths, raw_path)?;
            if !resolved.is_file() {
                return Err(AppError::bad_request(format!(
                    "path is not a file: {raw_path}"
                )));
            }
            let meta = tokio::fs::metadata(&resolved)
                .await
                .map_err(|e| AppError::bad_request(format!("cannot stat file: {e}")))?;
            if meta.len() > ctx.max_file_bytes as u64 {
                return Err(AppError::payload_too_large(format!(
                    "file is too large to publish as an artifact (max {} bytes)",
                    ctx.max_file_bytes
                )));
            }
            let data = tokio::fs::read(&resolved)
                .await
                .map_err(|e| AppError::bad_request(format!("cannot read file: {e}")))?;
            let filename = resolved
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "artifact".to_string());
            let content_type = crate::storage::guess_content_type(&filename);
            (filename, Some(content_type), Bytes::from(data))
        } else if let Some(content) = input.get("content").and_then(Value::as_str) {
            if content.len() > ARTIFACT_INLINE_CAP {
                return Err(AppError::payload_too_large(format!(
                    "content is too large for an inline artifact (max {ARTIFACT_INLINE_CAP} bytes)"
                )));
            }
            let filename = input
                .get("filename")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::bad_request("'filename' is required with 'content'"))?;
            // Inline content is text: default to text/plain unless the
            // extension says otherwise.
            let content_type = if Path::new(filename).extension().is_some() {
                crate::storage::guess_content_type(filename)
            } else {
                "text/plain".to_string()
            };
            (
                filename.to_string(),
                Some(content_type),
                Bytes::from(content.to_string()),
            )
        } else {
            return Err(AppError::bad_request(
                "provide either 'path' (existing file) or 'content' + 'filename'",
            ));
        };

        let artifact = files::register_artifact(
            &ctx.pool,
            &ctx.storage,
            ctx.user_id,
            ctx.conversation_id,
            &filename,
            content_type.as_deref(),
            data,
        )
        .await?;
        Ok(json!({
            "saved": true,
            "artifact": {
                "id": artifact.id,
                "filename": artifact.filename,
                "content_type": artifact.content_type,
                "size_bytes": artifact.size_bytes,
                "url": artifact.url,
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::fs::canonical_deepest;

    fn sandbox() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace").join("conv-1");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("report.md"), "hello").unwrap();
        (dir, root)
    }

    #[test]
    fn workspace_relative_path_is_allowed_in_local_mode() {
        let (_dir, root) = sandbox();
        let resolved = resolve_artifact_path(AppMode::Local, &root, &[], "report.md").unwrap();
        let expected = canonical_deepest(&root).unwrap().join("report.md");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn at_workspace_prefix_is_allowed() {
        let (_dir, root) = sandbox();
        let resolved =
            resolve_artifact_path(AppMode::Local, &root, &[], "@workspace/report.md").unwrap();
        assert_eq!(
            resolved,
            canonical_deepest(&root).unwrap().join("report.md")
        );
    }

    #[test]
    fn paths_outside_workspace_and_whitelist_are_rejected() {
        let (dir, root) = sandbox();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "nope").unwrap();
        let err = resolve_artifact_path(AppMode::Local, &root, &[], outside.to_str().unwrap())
            .unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::FORBIDDEN);
    }
}
