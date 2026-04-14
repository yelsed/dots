use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// Copy a file or directory from source to destination
pub fn copy_entry(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst)
            .with_context(|| format!("Failed to copy dir {} -> {}", src.display(), dst.display()))
    } else if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)
            .with_context(|| format!("Failed to copy {} -> {}", src.display(), dst.display()))?;
        Ok(())
    } else {
        anyhow::bail!("Source does not exist: {}", src.display())
    }
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // Skip .git directories
        if src_path.file_name().map(|n| n == ".git").unwrap_or(false) {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Check if two files/directories have different content
pub fn has_changes(a: &Path, b: &Path) -> Result<bool> {
    if a.is_file() && b.is_file() {
        Ok(file_hash(a)? != file_hash(b)?)
    } else if a.is_dir() && b.is_dir() {
        Ok(dir_diff(a, b)?.has_changes())
    } else if a.exists() != b.exists() {
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Get detailed diff for two directories, or None if not both directories
pub fn dir_diff_detail(a: &Path, b: &Path) -> Result<Option<DirDiffDetail>> {
    if a.is_dir() && b.is_dir() {
        Ok(Some(dir_diff(a, b)?))
    } else {
        Ok(None)
    }
}

/// Detail about how two directories differ
#[derive(Debug, Clone)]
pub struct DirDiffDetail {
    pub local_only: usize,
    pub repo_only: usize,
    pub content_changed: usize,
    pub local_only_files: Vec<std::path::PathBuf>,
    pub repo_only_files: Vec<std::path::PathBuf>,
    pub changed_files: Vec<std::path::PathBuf>,
}

impl DirDiffDetail {
    pub fn has_changes(&self) -> bool {
        self.local_only > 0 || self.repo_only > 0 || self.content_changed > 0
    }
}

/// Compare two directories recursively and return a breakdown of differences
fn dir_diff(a: &Path, b: &Path) -> Result<DirDiffDetail> {
    let a_entries = list_files_recursive(a)?;
    let b_entries = list_files_recursive(b)?;

    let a_relative: std::collections::HashSet<_> = a_entries
        .iter()
        .filter_map(|p| p.strip_prefix(a).ok())
        .map(|p| p.to_path_buf())
        .collect();
    let b_relative: std::collections::HashSet<_> = b_entries
        .iter()
        .filter_map(|p| p.strip_prefix(b).ok())
        .map(|p| p.to_path_buf())
        .collect();

    let mut local_only_files: Vec<_> = a_relative.difference(&b_relative).cloned().collect();
    let mut repo_only_files: Vec<_> = b_relative.difference(&a_relative).cloned().collect();
    local_only_files.sort();
    repo_only_files.sort();

    let mut changed_files = Vec::new();
    for rel_path in a_relative.intersection(&b_relative) {
        let a_file = a.join(rel_path);
        let b_file = b.join(rel_path);
        if file_hash(&a_file)? != file_hash(&b_file)? {
            changed_files.push(rel_path.clone());
        }
    }
    changed_files.sort();

    Ok(DirDiffDetail {
        local_only: local_only_files.len(),
        repo_only: repo_only_files.len(),
        content_changed: changed_files.len(),
        local_only_files,
        repo_only_files,
        changed_files,
    })
}

/// List all files in a directory recursively
fn list_files_recursive(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().map(|n| n == ".git").unwrap_or(false) {
            continue;
        }
        if path.is_dir() {
            files.extend(list_files_recursive(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

/// Most recent modification time for a file, or for the newest file within a directory (recursive).
/// Returns `None` if the path does not exist or contains no files.
pub fn last_modified(path: &Path) -> Result<Option<SystemTime>> {
    if !path.exists() {
        return Ok(None);
    }
    if path.is_file() {
        let meta = fs::metadata(path)
            .with_context(|| format!("Failed to stat {}", path.display()))?;
        return Ok(Some(meta.modified()?));
    }
    // Directory: walk and take the max mtime
    let files = list_files_recursive(path)?;
    let mut newest: Option<SystemTime> = None;
    for f in files {
        if let Ok(meta) = fs::metadata(&f) {
            if let Ok(m) = meta.modified() {
                newest = Some(match newest {
                    Some(prev) if prev >= m => prev,
                    _ => m,
                });
            }
        }
    }
    Ok(newest)
}

/// SHA256 hash of a file
fn file_hash(path: &Path) -> Result<String> {
    let content = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(hex::encode(hasher.finalize()))
}

/// Get a list of changed files for display
pub fn diff_summary(
    entries: &[crate::config::Entry],
    repo_root: &Path,
) -> Result<Vec<ChangedEntry>> {
    let mut changes = Vec::new();

    for entry in entries {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(repo_root);

        let (status, detail) = if !source.exists() && !repo_path.exists() {
            (ChangeStatus::Missing, None)
        } else if !source.exists() {
            (ChangeStatus::RepoOnly, None)
        } else if !repo_path.exists() {
            (ChangeStatus::SystemOnly, None)
        } else if has_changes(&source, &repo_path)? {
            let detail = dir_diff_detail(&source, &repo_path)?;
            (ChangeStatus::Modified, detail)
        } else {
            (ChangeStatus::Synced, None)
        };

        changes.push(ChangedEntry {
            entry: entry.clone(),
            status,
            detail,
        });
    }

    Ok(changes)
}

#[derive(Debug, Clone)]
pub struct ChangedEntry {
    pub entry: crate::config::Entry,
    pub status: ChangeStatus,
    pub detail: Option<DirDiffDetail>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChangeStatus {
    Synced,
    Modified,
    SystemOnly,
    RepoOnly,
    Missing,
}

impl std::fmt::Display for ChangeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeStatus::Synced => write!(f, "synced"),
            ChangeStatus::Modified => write!(f, "modified"),
            ChangeStatus::SystemOnly => write!(f, "system only"),
            ChangeStatus::RepoOnly => write!(f, "repo only"),
            ChangeStatus::Missing => write!(f, "missing"),
        }
    }
}
