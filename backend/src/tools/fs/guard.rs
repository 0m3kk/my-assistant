//! PathGuard — filesystem access control for fs tools.
//!
//! - `server` mode: everything is confined to the per-conversation sandbox
//!   (`DATA_DIR/workspace/<conversation-id>`). Absolute paths are allowed only
//!   when they already live inside the sandbox.
//! - `local` mode: only absolute paths inside the configured whitelist are
//!   accessible; an empty whitelist denies everything.
//!
//! Containment is enforced via canonicalization of the deepest existing
//! ancestor, so symlinks pointing outside are rejected as well.

use std::path::{Component, Path, PathBuf};

use crate::config::AppMode;

#[derive(Debug, Clone)]
pub struct PathGuard {
    pub mode: AppMode,
    pub sandbox_root: PathBuf,
    pub allowed_paths: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PathDenied(pub String);

impl PathGuard {
    pub fn new(mode: AppMode, sandbox_root: PathBuf, allowed_paths: Vec<PathBuf>) -> Self {
        Self {
            mode,
            sandbox_root,
            allowed_paths,
        }
    }

    /// Resolve a user/LLM supplied path to a checked absolute path.
    pub fn resolve(&self, raw: &str, for_write: bool) -> Result<PathBuf, PathDenied> {
        if raw.trim().is_empty() {
            return Err(PathDenied("path is empty".into()));
        }
        let candidate = PathBuf::from(raw);
        // Reject traversal components outright.
        if candidate
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(PathDenied("path must not contain '..'".into()));
        }

        match self.mode {
            AppMode::Server => self.resolve_server(&candidate, for_write),
            AppMode::Local => self.resolve_local(&candidate, for_write),
        }
    }

    fn resolve_server(&self, candidate: &Path, _for_write: bool) -> Result<PathBuf, PathDenied> {
        let base = canonical_deepest(&self.sandbox_root)
            .map_err(|e| PathDenied(format!("sandbox unavailable: {e}")))?;
        let target = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            base.join(candidate)
        };
        let canonical = canonical_under(&target, &base)?;
        if !canonical.starts_with(&base) {
            return Err(PathDenied("path is outside the workspace sandbox".into()));
        }
        Ok(canonical)
    }

    fn resolve_local(&self, candidate: &Path, _for_write: bool) -> Result<PathBuf, PathDenied> {
        if self.allowed_paths.is_empty() {
            return Err(PathDenied(
                "file access denied: no whitelisted paths configured for this agent".into(),
            ));
        }
        if !candidate.is_absolute() {
            return Err(PathDenied(
                "path must be absolute in local mode (whitelist roots define the allowed areas)"
                    .into(),
            ));
        }
        let canonical = canonical_deepest(candidate)
            .map_err(|e| PathDenied(format!("cannot resolve path: {e}")))?;
        for root in &self.allowed_paths {
            let root_canonical = canonical_deepest(root)
                .map_err(|e| PathDenied(format!("whitelist root unavailable: {e}")))?;
            if canonical.starts_with(&root_canonical) {
                return Ok(canonical);
            }
        }
        Err(PathDenied(
            "path is outside the whitelisted directories".into(),
        ))
    }
}

/// Canonicalize the deepest existing ancestor of `path` and re-attach the
/// remaining (non-existing) components.
pub fn canonical_deepest(path: &Path) -> std::io::Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut existing = abs.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(_) => {
                let last = existing.file_name().map(|s| s.to_os_string());
                match last {
                    Some(name) => {
                        tail.push(name);
                        if !existing.pop() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
    let mut canonical = existing.canonicalize()?;
    for comp in tail.into_iter().rev() {
        canonical.push(comp);
    }
    Ok(sanitize_components(canonical))
}

/// Canonicalize `path`, but if it does not exist yet, canonicalize the deepest
/// existing ancestor under `base` (for sandbox writes).
fn canonical_under(path: &Path, _base: &Path) -> Result<PathBuf, PathDenied> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|e| PathDenied(format!("cannot resolve path: {e}")));
    }
    canonical_deepest(path).map_err(|e| PathDenied(format!("cannot resolve path: {e}")))
}

/// Normalize away `.` and redundant separators after manual joining.
fn sanitize_components(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_sandbox() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace").join("conv-1");
        std::fs::create_dir_all(&root).unwrap();
        // Leak the tempdir for the test lifetime via a static-ish clone trick:
        std::mem::forget(dir);
        root
    }

    #[test]
    fn server_mode_confines_to_sandbox() {
        let root = temp_sandbox();
        let root_canonical = root.canonicalize().unwrap();
        let guard = PathGuard::new(AppMode::Server, root, vec![]);

        let inside = guard.resolve("notes/a.txt", true).unwrap();
        assert!(inside.starts_with(&root_canonical));

        let outside = guard.resolve("/etc/passwd", false);
        assert!(outside.is_err());
    }

    #[test]
    fn server_mode_rejects_traversal() {
        let root = temp_sandbox();
        let guard = PathGuard::new(AppMode::Server, root, vec![]);
        assert!(guard.resolve("../../etc/passwd", false).is_err());
        assert!(guard.resolve("a/../../b", true).is_err());
    }

    #[test]
    fn local_mode_requires_whitelist() {
        let guard = PathGuard::new(AppMode::Local, PathBuf::from("/unused"), vec![]);
        assert!(guard.resolve("/tmp/whatever", false).is_err());

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let guard = PathGuard::new(AppMode::Local, PathBuf::from("/unused"), vec![root.clone()]);

        let file = root.join("sub").join("file.txt");
        let resolved = guard.resolve(&file.to_string_lossy(), true).unwrap();
        // Only `root` exists — deepest-ancestor canonicalization keeps the tail.
        assert_eq!(
            resolved,
            root.canonicalize().unwrap().join("sub").join("file.txt")
        );

        let outside = std::env::temp_dir().join("not-whitelisted.txt");
        assert!(guard.resolve(&outside.to_string_lossy(), false).is_err());
        let _ = root;
    }

    #[test]
    fn symlink_escape_is_rejected() {
        let root = temp_sandbox();
        let escape_target = tempfile::tempdir().unwrap();
        std::mem::forget(escape_target);
        let link = root.join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/tmp", &link).unwrap();

        let guard = PathGuard::new(AppMode::Server, root, vec![]);
        let result = guard.resolve("escape/etc/hosts", false);
        assert!(result.is_err() || !result.unwrap().starts_with("/tmp"));
    }
}
