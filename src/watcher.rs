use anyhow::Result;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config::{DotsConfig, Entry};
use crate::git;
use crate::platform;
use crate::sync;

/// Run the file watcher daemon
pub fn run_watcher(
    config: &DotsConfig,
    repo_root: &PathBuf,
    poll_interval_mins: u64,
) -> Result<()> {
    let (tx, rx) = mpsc::channel::<Event>();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })?;

    // Watch all platform-relevant source paths
    let entries = config.platform_entries();
    let mut watched_paths: Vec<PathBuf> = Vec::new();

    for entry in &entries {
        let source = entry.expanded_source();
        if source.exists() {
            let mode = if source.is_dir() {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            if let Err(e) = watcher.watch(&source, mode) {
                eprintln!("Warning: could not watch {}: {}", source.display(), e);
            } else {
                watched_paths.push(source);
            }
        }
    }

    if watched_paths.is_empty() {
        anyhow::bail!("No paths to watch. Add some entries with 'dots add' first.");
    }

    println!("Watching {} paths for changes...", watched_paths.len());
    for p in &watched_paths {
        println!("  {}", p.display());
    }

    let debounce = Duration::from_secs(config.watch.debounce_secs);
    let poll_interval = Duration::from_secs(poll_interval_mins * 60);
    let mut last_sync = Instant::now();
    let mut last_poll = Instant::now();
    let mut pending_change = false;

    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            // Reading files (including our own hashing) raises access events; only writes count
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
            Ok(_event) => {
                pending_change = true;
                last_sync = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Debounce: if we have pending changes and enough time has passed
        if pending_change && last_sync.elapsed() >= debounce {
            pending_change = false;
            if let Err(e) = sync_and_push(config, repo_root, &entries) {
                eprintln!("Sync error: {}", e);
            }
        }

        // Remote poll
        if last_poll.elapsed() >= poll_interval {
            last_poll = Instant::now();
            if let Err(e) = check_remote(config, repo_root, &entries) {
                eprintln!("Remote check error: {}", e);
            }
        }
    }

    Ok(())
}

/// Copy changed files to repo, commit, and push
fn sync_and_push(config: &DotsConfig, repo_root: &Path, entries: &[Entry]) -> Result<()> {
    let mut changed_files = Vec::new();

    for entry in entries {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(repo_root);

        let ignore = &config.watch.ignore;
        if source.exists() && sync::has_changes(&source, &repo_path, ignore).unwrap_or(true) {
            sync::copy_entry(&source, &repo_path, ignore)?;
            changed_files.push(entry.repo_path.clone());
        }
    }

    if changed_files.is_empty() {
        return Ok(());
    }

    let message = if changed_files.len() == 1 {
        format!("sync: {}", changed_files[0])
    } else {
        format!("sync: {} files changed", changed_files.len())
    };

    println!("Committing: {}", message);

    let repo = git::open_repo(repo_root)?;
    git::commit_all(&repo, &message)?;

    match git::push(&repo, &config.repo.remote) {
        Ok(()) => println!("Pushed to {}", config.repo.remote),
        Err(e) => eprintln!("Push failed (will retry): {}", e),
    }

    Ok(())
}

/// Check if the remote has new commits. Without `auto_apply` only notify;
/// with it, pull and copy the auto-apply entries to the system, skipping any
/// entry that had local changes before the pull.
fn check_remote(config: &DotsConfig, repo_root: &Path, entries: &[Entry]) -> Result<()> {
    let remote_name = &config.repo.remote;
    let ignore = &config.watch.ignore;
    let repo = git::open_repo(repo_root)?;
    let git::RemoteStatus::Behind(count) = git::fetch_and_check(&repo, remote_name)? else {
        return Ok(());
    };

    if config.watch.auto_apply.is_empty() {
        let body = format!(
            "{} dotfile change{} available, run `dots pull` to apply",
            count,
            if count == 1 { "" } else { "s" }
        );
        send_notification("dots", &body);
        return Ok(());
    }

    let auto_apply: Vec<&Entry> = entries
        .iter()
        .filter(|entry| {
            config
                .watch
                .auto_apply
                .iter()
                .any(|path| crate::config::expand_tilde(path) == entry.expanded_source())
        })
        .collect();

    let auto_apply_repo_paths: Vec<&str> = auto_apply
        .iter()
        .map(|entry| entry.repo_path.as_str())
        .collect();

    // Decide before pulling: afterwards the repository no longer shows the local state
    let mut clean = Vec::new();
    let mut locally_changed = Vec::new();
    for entry in auto_apply {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(repo_root);
        if source.exists() && sync::has_changes(&source, &repo_path, ignore).unwrap_or(true) {
            locally_changed.push(entry);
        } else {
            clean.push(entry);
        }
    }

    let changed_files = git::changed_files(&repo, remote_name)?;
    git::pull(&repo, remote_name)?;
    println!(
        "Pulled {} commit{} from {}",
        count,
        if count == 1 { "" } else { "s" },
        remote_name
    );

    for entry in clean {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(repo_root);
        if repo_path.exists() && sync::has_changes(&repo_path, &source, ignore)? {
            sync::copy_entry(&repo_path, &source, ignore)?;
            send_notification("dots", &format!("Applied {}", entry.source));
        }
    }
    for entry in &locally_changed {
        send_notification(
            "dots",
            &format!(
                "Skipped {}: it has local changes that are not pushed yet",
                entry.source
            ),
        );
    }

    let other_changes = changed_files
        .iter()
        .filter(|file| {
            !auto_apply_repo_paths
                .iter()
                .any(|repo_path| Path::new(file).starts_with(repo_path))
        })
        .count();
    if other_changes > 0 {
        send_notification(
            "dots",
            &format!(
                "{} other file{} pulled, run `dots link` to apply",
                other_changes,
                if other_changes == 1 { "" } else { "s" }
            ),
        );
    }

    Ok(())
}

/// Send a desktop notification
fn send_notification(summary: &str, body: &str) {
    if platform::Platform::current() == platform::Platform::Linux {
        // Use notify-rust for Linux (D-Bus)
        let _ = notify_rust::Notification::new()
            .summary(summary)
            .body(body)
            .timeout(10000)
            .show();
    } else if platform::Platform::current() == platform::Platform::Macos {
        // Use osascript for macOS
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            body.replace('"', "\\\""),
            summary.replace('"', "\\\"")
        );
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output();
    }
    // Always print to terminal too
    println!("[notify] {} — {}", summary, body);
}
