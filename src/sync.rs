use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

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
        dir_has_changes(a, b)
    } else if a.exists() != b.exists() {
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Compare two directories recursively
fn dir_has_changes(a: &Path, b: &Path) -> Result<bool> {
    let a_entries = list_files_recursive(a)?;
    let b_entries = list_files_recursive(b)?;

    // Check if file sets differ
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

    if a_relative != b_relative {
        return Ok(true);
    }

    // Check content of matching files
    for rel_path in &a_relative {
        let a_file = a.join(rel_path);
        let b_file = b.join(rel_path);
        if file_hash(&a_file)? != file_hash(&b_file)? {
            return Ok(true);
        }
    }

    Ok(false)
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

/// SHA256 hash of a file
fn file_hash(path: &Path) -> Result<String> {
    let content = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(hex::encode(hasher.finalize()))
}

/// Get a list of changed files for display
pub fn diff_summary(
    entries: &[&crate::config::Entry],
    repo_root: &Path,
) -> Result<Vec<ChangedEntry>> {
    let mut changes = Vec::new();

    for entry in entries {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(repo_root);

        let status = if !source.exists() && !repo_path.exists() {
            ChangeStatus::Missing
        } else if !source.exists() {
            ChangeStatus::RepoOnly
        } else if !repo_path.exists() {
            ChangeStatus::SystemOnly
        } else if has_changes(&source, &repo_path)? {
            ChangeStatus::Modified
        } else {
            ChangeStatus::Synced
        };

        changes.push(ChangedEntry {
            entry: (*entry).clone(),
            status,
        });
    }

    Ok(changes)
}

#[derive(Debug, Clone)]
pub struct ChangedEntry {
    pub entry: crate::config::Entry,
    pub status: ChangeStatus,
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
