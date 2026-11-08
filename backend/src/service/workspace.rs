//! Workspace exploration: listing the per-conversation sandbox tree.
//!
//! The sandbox filesystem is the source of truth — nothing is tracked in the
//! database. Listing is best-effort: unreadable entries are skipped.

use std::path::Path;

use chrono::DateTime;
use tokio::task::spawn_blocking;

use crate::domain::WorkspaceEntry;

/// Hard caps so a huge/recursive sandbox cannot stall the API.
const MAX_ENTRIES: usize = 2000;
const MAX_DEPTH: usize = 16;

/// Recursively list the sandbox tree (directories first, then files, each
/// group sorted by name). Returns paths relative to `root`, `/`-separated.
pub async fn list_tree(root: &Path) -> Vec<WorkspaceEntry> {
    let root = root.to_path_buf();
    spawn_blocking(move || walk(&root, Path::new(""), 0, MAX_ENTRIES))
        .await
        .unwrap_or_default()
}

fn walk(root: &Path, relative: &Path, depth: usize, budget: usize) -> Vec<WorkspaceEntry> {
    if depth > MAX_DEPTH || budget == 0 {
        return Vec::new();
    }
    let dir_path = if relative.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    let Ok(read_dir) = std::fs::read_dir(&dir_path) else {
        return Vec::new();
    };
    let mut dirs: Vec<WorkspaceEntry> = Vec::new();
    let mut files: Vec<WorkspaceEntry> = Vec::new();
    let mut remaining = budget;
    for entry in read_dir.flatten() {
        if remaining == 0 {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_rel = relative.join(&name);
        let rel = child_rel.to_string_lossy().replace('\\', "/");
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue, // raced / unreadable — skip
        };
        remaining -= 1;
        if file_type.is_symlink() {
            // Never follow or expose symlinks: containment guarantee.
            continue;
        }
        let modified = meta.modified().ok().and_then(|t| {
            let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
            DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
        });
        if file_type.is_dir() {
            dirs.push(WorkspaceEntry {
                path: rel,
                is_dir: true,
                size_bytes: None,
                modified_at: modified,
            });
        } else {
            files.push(WorkspaceEntry {
                path: rel,
                is_dir: false,
                size_bytes: Some(meta.len() as i64),
                modified_at: modified,
            });
        }
    }
    let mut out = Vec::with_capacity(dirs.len() + files.len());
    dirs.sort_by(|a, b| a.path.cmp(&b.path));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    out.extend(dirs);
    out.extend(files);
    if remaining > 0 && depth < MAX_DEPTH {
        let sub_dirs: Vec<String> = out
            .iter()
            .filter(|e| e.is_dir)
            .map(|e| e.path.clone())
            .collect();
        for sub in sub_dirs {
            if remaining == 0 {
                break;
            }
            let child = walk(root, Path::new(&sub), depth + 1, remaining);
            remaining = remaining.saturating_sub(child.len());
            out.extend(child);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_tree_recursively_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::write(root.join("b.txt"), "b").unwrap();
        std::fs::write(root.join("src/a.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/nested/deep.txt"), "deep").unwrap();

        let entries = walk(root, Path::new(""), 0, MAX_ENTRIES);
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        // DFS: current dir's entries (dirs first), then each subdir's contents.
        assert_eq!(
            paths,
            vec![
                "src",
                "b.txt",
                "src/nested",
                "src/a.rs",
                "src/nested/deep.txt"
            ]
        );
        assert!(entries[0].is_dir);
        let file = entries.iter().find(|e| e.path == "src/a.rs").unwrap();
        assert_eq!(file.size_bytes, Some(12)); // "fn main() {}" = 12 bytes
    }

    #[test]
    fn budget_limits_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..10 {
            std::fs::write(root.join(format!("f{i}.txt")), "x").unwrap();
        }
        let entries = walk(root, Path::new(""), 0, 3);
        assert_eq!(entries.len(), 3);
    }
}
