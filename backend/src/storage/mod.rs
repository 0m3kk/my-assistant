//! Pluggable object storage for user-facing files (attachments + artifacts).
//!
//! Two backends ship today: [`local::LocalStorage`] (filesystem under
//! `DATA_DIR/files`) and [`s3::S3Storage`] (any S3-compatible API, selected
//! via `STORAGE_BACKEND`). Conversation sandbox ("workspace") files are
//! deliberately NOT routed through this trait — fs tools need real paths and
//! the sandbox is ephemeral scratch space (see `tools::fs`).

pub mod local;
pub mod s3;

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

use crate::config::{Config, StorageBackend};

/// Stream of object bytes returned by [`Storage::get`].
pub type StorageStream = Pin<Box<dyn futures::Stream<Item = Result<Bytes, StorageError>> + Send>>;

/// Storage failures safe to surface as problem+json responses.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("object not found: {0}")]
    NotFound(String),
    #[error("storage io error: {0}")]
    Io(String),
    #[error("storage backend error: {0}")]
    Backend(String),
}

/// Object storage abstraction: put / stream / delete by opaque key.
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    /// Store `data` at `key`, creating any needed parent structure.
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StorageError>;
    /// Stream the object's bytes back.
    async fn get(&self, key: &str) -> Result<StorageStream, StorageError>;
    /// Remove the object. Deleting a missing object is not an error.
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
}

/// Build the storage backend selected by configuration.
pub fn create_storage(cfg: &Config) -> Result<Arc<dyn Storage>, String> {
    match cfg.storage_backend {
        StorageBackend::Local => {
            let root = cfg.storage_root();
            std::fs::create_dir_all(&root)
                .map_err(|e| format!("cannot create storage root {}: {e}", root.display()))?;
            Ok(Arc::new(local::LocalStorage::new(root)))
        }
        StorageBackend::S3 => Ok(Arc::new(s3::S3Storage::from_config(cfg)?)),
    }
}

/// Build the canonical storage key for a file row:
/// `files/{file_id}/{sanitized filename}` — stable regardless of when the
/// file is linked to a conversation.
pub fn file_key(file_id: uuid::Uuid, filename: &str) -> String {
    format!("files/{file_id}/{}", sanitize_filename(filename))
}

/// Normalize an untrusted filename: keep the basename, strip path/control
/// characters, cap the length, and always yield something non-empty.
pub fn sanitize_filename(raw: &str) -> String {
    let name = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let mut out: String = name
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .take(200)
        .collect();
    out = out.trim().trim_matches('.').to_string();
    if out.is_empty() {
        "file".to_string()
    } else {
        out
    }
}

/// Guess a content type from a filename, defaulting to a safe octet-stream.
pub fn guess_content_type(filename: &str) -> String {
    mime_guess::from_path(filename)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_basename_and_strips_traversal() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a\\b\\c.txt"), "c.txt");
        assert_eq!(sanitize_filename(".."), "file");
        assert_eq!(sanitize_filename(""), "file");
        assert_eq!(sanitize_filename("weird:na*me?.md"), "weirdname.md");
    }

    #[test]
    fn sanitize_caps_length() {
        let long = format!("{a}.txt", a = "x".repeat(500));
        let out = sanitize_filename(&long);
        assert!(out.len() <= 210); // 200 chars + ".txt" tail is not guaranteed, cap is on chars
        assert!(out.chars().count() <= 200);
    }

    #[test]
    fn key_uses_file_id() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            file_key(id, "a b.txt"),
            "files/00000000-0000-0000-0000-000000000000/a b.txt"
        );
    }

    #[test]
    fn guess_defaults_to_octet_stream() {
        assert_eq!(guess_content_type("a.png"), "image/png");
        assert_eq!(
            guess_content_type("noextension"),
            "application/octet-stream"
        );
    }
}
