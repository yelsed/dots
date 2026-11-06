use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Copy a file or directory from source to destination. For a folder, the
/// destination is made to mirror the source: names in `ignore` are skipped on
/// both sides, links are recreated rather than followed, and anything in the
/// destination that no longer exists in the source is removed.
pub fn copy_entry(src: &Path, dst: &Path, ignore: &[String]) -> Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst, ignore).with_context(|| {
            format!("Failed to copy dir {} -> {}", src.display(), dst.display())
        })?;
        remove_stale(src, dst, ignore)
            .with_context(|| format!("Failed to remove stale files in {}", dst.display()))
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

/// What a path inside a folder entry is, without following links.
enum Kind {
    File,
    Dir,
    Link(PathBuf),
}

/// Classify a child of a folder entry, or `None` when it must be left alone:
/// ignored names, `.git`, and links that point to an absolute path outside
/// `$HOME` (those belong to the machine, such as Omarchy package skills).
fn classify(path: &Path, ignore: &[String]) -> Result<Option<Kind>> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if name == ".git" || ignore.iter().any(|ignored| *ignored == name) {
        return Ok(None);
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(path)?;
        if is_machine_local_link(&target) {
            return Ok(None);
        }
        return Ok(Some(Kind::Link(target)));
    }
    Ok(Some(if meta.is_dir() { Kind::Dir } else { Kind::File }))
}

fn is_machine_local_link(target: &Path) -> bool {
    target.is_absolute()
        && !dirs::home_dir()
            .map(|home| target.starts_with(home))
            .unwrap_or(false)
}

/// Remove whatever sits at `path` (file, link or folder), if anything.
fn remove_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(_) => {}
    }
    Ok(())
}

#[cfg(unix)]
fn create_link(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("Failed to create link {}", link.display()))
}

#[cfg(not(unix))]
fn create_link(_target: &Path, link: &Path) -> Result<()> {
    eprintln!(
        "Warning: skipping link {} (links are not supported on this platform)",
        link.display()
    );
    Ok(())
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path, ignore: &[String]) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let src_path = entry?.path();
        let dst_path = dst.join(src_path.file_name().unwrap_or_default());
        let existing = fs::symlink_metadata(&dst_path).ok();

        match classify(&src_path, ignore)? {
            None => {}
            Some(Kind::Link(target)) => {
                if fs::read_link(&dst_path).ok().as_deref() == Some(target.as_path()) {
                    continue;
                }
                remove_path(&dst_path)?;
                create_link(&target, &dst_path)?;
            }
            Some(Kind::Dir) => {
                if existing.is_some_and(|meta| !meta.is_dir()) {
                    remove_path(&dst_path)?;
                }
                copy_dir_recursive(&src_path, &dst_path, ignore)?;
            }
            Some(Kind::File) => {
                // fs::copy would write through a link, and fails on a folder
                if existing.is_some_and(|meta| !meta.is_file()) {
                    remove_path(&dst_path)?;
                }
                fs::copy(&src_path, &dst_path)?;
            }
        }
    }
    Ok(())
}

/// Remove files, links and folders in `dst` that no longer exist in `src`.
/// Ignored names and machine-local links in `dst` are never touched.
fn remove_stale(src: &Path, dst: &Path, ignore: &[String]) -> Result<()> {
    for entry in fs::read_dir(dst)? {
        let dst_path = entry?.path();
        let src_path = src.join(dst_path.file_name().unwrap_or_default());

        let Some(kind) = classify(&dst_path, ignore)? else {
            continue;
        };
        let source_kind = if fs::symlink_metadata(&src_path).is_ok() {
            classify(&src_path, ignore)?
        } else {
            None
        };
        match (kind, source_kind) {
            (Kind::Dir, Some(Kind::Dir)) => remove_stale(&src_path, &dst_path, ignore)?,
            (_, Some(_)) => {}
            (Kind::Dir, None) => {
                // Keep the folder if it still holds ignored names or machine-local links
                remove_stale(&src_path, &dst_path, ignore)?;
                let _ = fs::remove_dir(&dst_path);
            }
            (_, None) => fs::remove_file(&dst_path)?,
        }
    }
    Ok(())
}

/// Check if two files/directories have different content
pub fn has_changes(a: &Path, b: &Path, ignore: &[String]) -> Result<bool> {
    if a.is_file() && b.is_file() {
        Ok(file_hash(a)? != file_hash(b)?)
    } else if a.is_dir() && b.is_dir() {
        Ok(dir_diff(a, b, ignore)?.has_changes())
    } else if a.exists() != b.exists() {
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Get detailed diff for two directories, or None if not both directories
pub fn dir_diff_detail(a: &Path, b: &Path, ignore: &[String]) -> Result<Option<DirDiffDetail>> {
    if a.is_dir() && b.is_dir() {
        Ok(Some(dir_diff(a, b, ignore)?))
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
    pub local_only_files: Vec<PathBuf>,
    pub repo_only_files: Vec<PathBuf>,
    pub changed_files: Vec<PathBuf>,
}

impl DirDiffDetail {
    pub fn has_changes(&self) -> bool {
        self.local_only > 0 || self.repo_only > 0 || self.content_changed > 0
    }
}

/// Compare two directories recursively and return a breakdown of differences
fn dir_diff(a: &Path, b: &Path, ignore: &[String]) -> Result<DirDiffDetail> {
    let a_entries = list_files_recursive(a, ignore)?;
    let b_entries = list_files_recursive(b, ignore)?;

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
        if entry_fingerprint(&a.join(rel_path))? != entry_fingerprint(&b.join(rel_path))? {
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

/// Content hash for a file, or the target for a link, so a link never equals a file
fn entry_fingerprint(path: &Path) -> Result<String> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        Ok(format!("link:{}", fs::read_link(path)?.display()))
    } else {
        file_hash(path)
    }
}

/// List all files and links in a directory recursively, without following links
fn list_files_recursive(dir: &Path, ignore: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        match classify(&path, ignore)? {
            None => {}
            Some(Kind::Dir) => files.extend(list_files_recursive(&path, ignore)?),
            Some(_) => files.push(path),
        }
    }
    Ok(files)
}

/// Most recent modification time for a file, or for the newest file within a directory (recursive).
/// Returns `None` if the path does not exist or contains no files.
pub fn last_modified(path: &Path, ignore: &[String]) -> Result<Option<SystemTime>> {
    if !path.exists() {
        return Ok(None);
    }
    if path.is_file() {
        let meta =
            fs::metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
        return Ok(Some(meta.modified()?));
    }
    // Directory: walk and take the max mtime
    let files = list_files_recursive(path, ignore)?;
    let mut newest: Option<SystemTime> = None;
    for file in files {
        if let Ok(meta) = fs::symlink_metadata(&file) {
            if let Ok(modified) = meta.modified() {
                newest = Some(match newest {
                    Some(prev) if prev >= modified => prev,
                    _ => modified,
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
    ignore: &[String],
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
        } else if has_changes(&source, &repo_path, ignore)? {
            let detail = dir_diff_detail(&source, &repo_path, ignore)?;
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn copy_keeps_links_skips_ignored_and_removes_deleted() {
        let root = std::env::temp_dir().join(format!("dots-sync-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(source.join("skill")).unwrap();
        fs::create_dir_all(source.join(".trash")).unwrap();
        fs::write(source.join("skill/SKILL.md"), "skill").unwrap();
        fs::write(source.join(".trash/old.md"), "old").unwrap();
        symlink("skill", source.join("relative-link")).unwrap();
        symlink("/nonexistent-dots-target", source.join("absolute-link")).unwrap();
        let ignore = vec![".trash".to_string()];

        copy_entry(&source, &destination, &ignore).unwrap();

        let copied_link = destination.join("relative-link");
        assert!(fs::symlink_metadata(&copied_link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&copied_link).unwrap(), Path::new("skill"));
        assert!(fs::symlink_metadata(destination.join("absolute-link")).is_err());
        assert!(!destination.join(".trash").exists());
        assert!(!has_changes(&source, &destination, &ignore).unwrap());

        // A machine-local link and an ignored folder on the destination side survive deletion
        symlink("/nonexistent-dots-target", destination.join("machine-link")).unwrap();
        fs::create_dir_all(destination.join(".trash")).unwrap();

        fs::remove_dir_all(source.join("skill")).unwrap();
        fs::remove_file(source.join("relative-link")).unwrap();
        copy_entry(&source, &destination, &ignore).unwrap();

        assert!(!destination.join("skill").exists());
        assert!(fs::symlink_metadata(&copied_link).is_err());
        assert!(fs::symlink_metadata(destination.join("machine-link")).is_ok());
        assert!(destination.join(".trash").exists());

        fs::remove_dir_all(&root).unwrap();
    }
}
