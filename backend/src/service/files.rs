//! File services: multipart extraction, attachment linking + sandbox
//! materialization, artifact registration, and inline payload resolution for
//! LLM history assembly.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::Multipart;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use uuid::Uuid;

use crate::domain::FileRow;
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::storage::{self, Storage};

/// Text-like attachments up to this size are inlined into the prompt.
pub const TEXT_INLINE_CAP: i64 = 64 * 1024;
/// Images up to this size are embedded as data URLs for vision models.
pub const IMAGE_INLINE_CAP: i64 = 8 * 1024 * 1024;
/// Cap for text content passed to the `save_artifact` tool inline.
pub const ARTIFACT_INLINE_CAP: usize = 1024 * 1024;

/// Extensions whose files are treated as text for prompt inlining.
const TEXT_EXTENSIONS: &[&str] = &[
    "json",
    "csv",
    "tsv",
    "md",
    "markdown",
    "txt",
    "text",
    "log",
    "xml",
    "yml",
    "yaml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "properties",
    "py",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "rs",
    "go",
    "java",
    "kt",
    "kts",
    "c",
    "h",
    "cpp",
    "hpp",
    "cc",
    "cs",
    "rb",
    "php",
    "swift",
    "sh",
    "bash",
    "zsh",
    "fish",
    "sql",
    "html",
    "htm",
    "css",
    "scss",
    "less",
    "svg",
    "diff",
    "patch",
    "env",
    "gitignore",
    "dockerfile",
    "makefile",
    "gradle",
    "proto",
    "graphql",
    "vue",
    "svelte",
];

fn extension_of(filename: &str) -> Option<String> {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let ext = name.rsplit_once('.').map(|(_, ext)| ext)?;
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_lowercase())
}

/// Whether an attachment is text-like (safe + useful to inline into a prompt).
pub fn is_text_like(content_type: &str, filename: &str) -> bool {
    match content_type {
        "application/json"
        | "application/xml"
        | "application/yaml"
        | "application/toml"
        | "application/javascript"
        | "application/typescript"
        | "application/x-sh"
        | "application/x-yaml"
        | "application/sql"
        | "application/rtf" => true,
        ct if ct.starts_with("text/") => true,
        _ => {
            let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
            let base = name.to_lowercase();
            // Extension match, or full-name match for extensionless files
            // (Dockerfile, Makefile) and dotfiles (.gitignore, .env).
            extension_of(filename).is_some_and(|ext| TEXT_EXTENSIONS.contains(&ext.as_str()))
                || TEXT_EXTENSIONS.contains(&base.as_str())
        }
    }
}

// ---------- Upload ----------

/// One uploaded file extracted from a multipart body.
pub struct UploadedFile {
    pub filename: String,
    pub content_type: String,
    pub data: Bytes,
}

/// Extract the first file field from a multipart body, enforcing the size cap
/// while streaming (never buffers more than `max_bytes`).
pub async fn extract_upload(mut multipart: Multipart, max_bytes: usize) -> AppResult<UploadedFile> {
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("invalid multipart body: {e}")))?
    {
        let filename = field
            .file_name()
            .map(storage::sanitize_filename)
            .unwrap_or_else(|| "file".to_string());
        if field.name() != Some("file") && field.file_name().is_none() {
            continue; // skip non-file fields (e.g. extra metadata)
        }

        let content_type = field
            .content_type()
            .map(str::trim)
            .filter(|ct| !ct.is_empty() && ct.bytes().all(|b| (33..127).contains(&b)))
            .map(str::to_string)
            .unwrap_or_else(|| storage::guess_content_type(&filename));

        let mut buf = BytesMut::new();
        loop {
            let chunk = field
                .chunk()
                .await
                .map_err(|e| AppError::bad_request(format!("read upload failed: {e}")))?;
            let Some(chunk) = chunk else { break };
            if buf.len() + chunk.len() > max_bytes {
                return Err(AppError::payload_too_large(format!(
                    "file exceeds the maximum upload size of {} bytes",
                    max_bytes
                )));
            }
            buf.extend_from_slice(&chunk);
        }

        return Ok(UploadedFile {
            filename,
            content_type,
            data: buf.freeze(),
        });
    }
    Err(AppError::bad_request(
        "multipart body must contain a 'file' field",
    ))
}

/// Persist an uploaded file into storage + `files` table. The row starts
/// unlinked (`conversation_id IS NULL`) and is linked when the message is sent.
pub async fn store_attachment(
    state: &crate::state::AppState,
    user_id: Uuid,
    upload: UploadedFile,
) -> AppResult<FileRow> {
    let id = Uuid::new_v4();
    let key = storage::file_key(id, &upload.filename);
    state
        .storage
        .put(&key, upload.data.clone())
        .await
        .map_err(|e| AppError::internal(format!("storing upload failed: {e}")))?;
    let checksum = {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(&upload.data);
        hex::encode(digest)
    };
    let row = repo::files::insert(
        &state.pool,
        repo::files::NewFile {
            id,
            conversation_id: None,
            message_id: None,
            user_id,
            origin: "attachment".to_string(),
            filename: upload.filename,
            content_type: upload.content_type,
            size_bytes: upload.data.len() as i64,
            storage_key: key,
            checksum_sha256: Some(checksum),
        },
    )
    .await?;
    repo::audit::insert(
        &state.pool,
        Some(user_id),
        "file.upload",
        &row.filename,
        true,
        serde_json::json!({ "size": row.size_bytes, "content_type": row.content_type }),
    )
    .await
    .ok();
    Ok(row)
}

// ---------- Download ----------

/// Fetch an object fully into memory (uploads/artifacts are size-capped).
pub async fn read_all(storage: &Arc<dyn Storage>, key: &str) -> AppResult<Bytes> {
    let mut stream = storage
        .get(key)
        .await
        .map_err(|e| AppError::internal(format!("storage read failed: {e}")))?;
    let mut buf = BytesMut::new();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(
            &chunk.map_err(|e| AppError::internal(format!("storage read failed: {e}")))?,
        );
    }
    Ok(buf.freeze())
}

// ---------- Attachment linking ----------

/// Extract file ids referenced by `file` parts in a message.
pub fn file_ids_in_message(parts: &[serde_json::Value]) -> Vec<Uuid> {
    let message = crate::domain::ui_message::UIMessage {
        id: String::new(),
        role: "user".to_string(),
        parts: parts.to_vec(),
        metadata: None,
    };
    message
        .file_parts()
        .into_iter()
        .map(|fp| fp.file_id)
        .collect()
}

/// Link attachments referenced by the new user message to the conversation and
/// materialize them into the sandbox so fs tools can work with them.
pub async fn activate_attachments(
    state: &crate::state::AppState,
    user_id: Uuid,
    conversation_id: Uuid,
    message_id: Uuid,
    parts: &[serde_json::Value],
    sandbox_root: &std::path::Path,
) -> AppResult<()> {
    for file_id in file_ids_in_message(parts) {
        // Ownership check: 404 for files that are not the sender's.
        let row = repo::files::owned_by_id(&state.pool, file_id, user_id).await?;
        let linked = repo::files::link(&state.pool, file_id, conversation_id, message_id).await?;
        if linked {
            repo::audit::insert(
                &state.pool,
                Some(user_id),
                "file.attach",
                &row.filename,
                true,
                serde_json::json!({ "conversation_id": conversation_id }),
            )
            .await
            .ok();
        }
        // Materialize only into the conversation that owns the attachment
        // (fresh link, or re-post of the same conversation's transcript).
        if linked || row.conversation_id == Some(conversation_id) {
            materialize_into_sandbox(state, &row, sandbox_root).await?;
        }
    }
    Ok(())
}

/// Copy an attachment into the conversation sandbox
/// (`uploads/{file_id prefix}-{filename}`) so fs tools can read it. Idempotent.
async fn materialize_into_sandbox(
    state: &crate::state::AppState,
    row: &FileRow,
    sandbox_root: &std::path::Path,
) -> AppResult<()> {
    let short_id = row.id.simple().to_string()[..8].to_string();
    let target = sandbox_root
        .join("uploads")
        .join(format!("{short_id}-{}", row.filename));
    if tokio::fs::try_exists(&target).await.unwrap_or(false) {
        return Ok(());
    }
    let data = read_all(&state.storage, &row.storage_key).await?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::internal(format!("cannot create uploads dir: {e}")))?;
    }
    tokio::fs::write(&target, data)
        .await
        .map_err(|e| AppError::internal(format!("cannot materialize attachment: {e}")))?;
    Ok(())
}

// ---------- Artifacts ----------

/// Reference to a registered artifact, embedded in tool output for the UI.
pub struct ArtifactRef {
    pub id: Uuid,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub url: String,
}

/// Register a file as an artifact: copy into storage + insert a `files` row.
/// Used by the `save_artifact` tool (and future generation tools).
pub async fn register_artifact(
    pool: &sqlx::PgPool,
    storage: &Arc<dyn Storage>,
    user_id: Uuid,
    conversation_id: Uuid,
    filename: &str,
    content_type: Option<&str>,
    data: Bytes,
) -> AppResult<ArtifactRef> {
    let id = Uuid::new_v4();
    let filename = storage::sanitize_filename(filename);
    let content_type = content_type
        .filter(|ct| !ct.is_empty() && ct.bytes().all(|b| (33..127).contains(&b)))
        .map(str::to_string)
        .unwrap_or_else(|| storage::guess_content_type(&filename));
    let key = storage::file_key(id, &filename);
    storage
        .put(&key, data.clone())
        .await
        .map_err(|e| AppError::internal(format!("storing artifact failed: {e}")))?;
    let row = repo::files::insert(
        pool,
        repo::files::NewFile {
            id,
            conversation_id: Some(conversation_id),
            message_id: None,
            user_id,
            origin: "artifact".to_string(),
            filename: filename.clone(),
            content_type: content_type.clone(),
            size_bytes: data.len() as i64,
            storage_key: key,
            checksum_sha256: None,
        },
    )
    .await?;
    repo::audit::insert(
        pool,
        Some(user_id),
        "artifact.register",
        &filename,
        true,
        serde_json::json!({ "conversation_id": conversation_id, "size": row.size_bytes }),
    )
    .await
    .ok();
    Ok(ArtifactRef {
        id: row.id,
        filename,
        content_type,
        size_bytes: row.size_bytes,
        url: format!("/api/files/{}/content", row.id),
    })
}

// ---------- Inline payloads for LLM history ----------

/// Pre-resolved attachment content for history assembly (see
/// `agent::history_from_messages`).
#[derive(Debug, Clone)]
pub enum InlinePayload {
    /// Text-like file content, inlined as a fenced block.
    Text(String),
    /// Image encoded as a data URL (vision models only).
    ImageUrl(String),
}

fn to_data_url(content_type: &str, data: &[u8]) -> String {
    format!("data:{content_type};base64,{}", BASE64.encode(data))
}

/// Resolve inline payloads for a set of file rows. Images become data URLs
/// only when the model is vision-capable; text-like files become text. Files
/// beyond the caps resolve to no payload (history will show a note instead).
pub async fn resolve_inline_payloads(
    storage: &Arc<dyn Storage>,
    files: &[FileRow],
    vision: bool,
) -> HashMap<Uuid, InlinePayload> {
    let mut out = HashMap::new();
    for row in files {
        let is_image = row.content_type.starts_with("image/");
        if is_image {
            if !vision || row.size_bytes > IMAGE_INLINE_CAP {
                continue;
            }
            if let Ok(data) = read_all(storage, &row.storage_key).await {
                out.insert(
                    row.id,
                    InlinePayload::ImageUrl(to_data_url(&row.content_type, &data)),
                );
            }
            continue;
        }
        if is_text_like(&row.content_type, &row.filename) && row.size_bytes <= TEXT_INLINE_CAP {
            if let Ok(data) = read_all(storage, &row.storage_key).await {
                out.insert(
                    row.id,
                    InlinePayload::Text(String::from_utf8_lossy(&data).into_owned()),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_like_detection() {
        assert!(is_text_like("text/plain", "notes.txt"));
        assert!(is_text_like("application/json", "data.json"));
        assert!(is_text_like("application/octet-stream", "main.rs"));
        assert!(is_text_like("application/octet-stream", "Dockerfile"));
        assert!(!is_text_like("application/octet-stream", "photo.png"));
        assert!(!is_text_like("image/png", "photo.png"));
        assert!(!is_text_like("application/octet-stream", "archive.zip"));
    }

    #[test]
    fn file_ids_scan_parts() {
        let id = Uuid::new_v4();
        let parts = vec![
            serde_json::json!({"type":"text","text":"hi"}),
            serde_json::json!({"type":"file","mediaType":"image/png","url":format!("/api/files/{id}/content")}),
            serde_json::json!({"type":"file","mediaType":"image/png","url":"https://x/y"}),
        ];
        assert_eq!(file_ids_in_message(&parts), vec![id]);
    }
}
