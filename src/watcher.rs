use anyhow::Result;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config::{DotsConfig, Entry};
use crate::git;
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
    // Entries pulled but not applied to the system: pushing them would revert the remote
    let mut unapplied: Vec<PathBuf> = Vec::new();
    // Remote commit already notified about, so a waiting change is announced once, not every poll
    let mut notified_remote: Option<git2::Oid> = None;

    for path in &config.watch.auto_apply {
        let expanded = crate::config::expand_tilde(path);
        if !entries
            .iter()
            .any(|entry| entry.expanded_source() == expanded)
        {
            eprintln!("Warning: auto_apply {} matches no tracked entry", path);
        }
    }

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
            if let Err(e) = sync_and_push(config, repo_root, &entries, &mut unapplied) {
                eprintln!("Sync error: {}", e);
            }
        }

        // Remote poll
        if last_poll.elapsed() >= poll_interval {
            last_poll = Instant::now();
            match check_remote(config, repo_root, &entries, &mut notified_remote) {
                Ok(failed) => unapplied.extend(failed),
                Err(e) => eprintln!("Remote check error: {}", e),
            }
        }
    }

    Ok(())
}

/// Copy changed files to repo, commit, and push
fn sync_and_push(
    config: &DotsConfig,
    repo_root: &Path,
    entries: &[Entry],
    unapplied: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut changed_files = Vec::new();
    let ignore = &config.watch.ignore;

    unapplied.retain(|source| {
        entries
            .iter()
            .find(|entry| entry.expanded_source() == *source)
            .is_some_and(|entry| {
                sync::has_changes(source, &entry.full_repo_path(repo_root), ignore).unwrap_or(true)
            })
    });

    for entry in entries {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(repo_root);

        if unapplied.contains(&source) {
            continue;
        }
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

/// Check if the remote has new commits. Without `auto_apply` only notify.
/// With it, pull and copy the changed auto-apply entries to the system, but
/// only when nothing local would be overwritten or pushed back over the
/// remote: otherwise notify and leave it to `dots pull`.
/// Returns the sources that were pulled but could not be applied.
fn check_remote(
    config: &DotsConfig,
    repo_root: &Path,
    entries: &[Entry],
    notified_remote: &mut Option<git2::Oid>,
) -> Result<Vec<PathBuf>> {
    let remote_name = &config.repo.remote;
    let ignore = &config.watch.ignore;
    let repo = git::open_repo(repo_root)?;
    let git::RemoteStatus::Behind(count) = git::fetch_and_check(&repo, remote_name)? else {
        return Ok(Vec::new());
    };
    let remote_head = git::remote_head(&repo, remote_name)?;
    let first_notice = notified_remote.replace(remote_head) != Some(remote_head);
    let ask_for_manual_pull = |reason: &str| {
        if !first_notice {
            return Ok(Vec::new());
        }
        let body = format!(
            "{} dotfile change{} available, run `dots pull` to apply{}",
            count,
            if count == 1 { "" } else { "s" },
            reason
        );
        send_notification("dots", &body, Some("pull"));
        Ok(Vec::new())
    };

    if config.watch.auto_apply.is_empty() {
        return ask_for_manual_pull("");
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
    if auto_apply.is_empty() {
        return ask_for_manual_pull(" (no auto_apply path matches a tracked entry)");
    }
    if git::has_uncommitted_changes(&repo)? {
        return ask_for_manual_pull(" (the dotfiles repository has uncommitted changes)");
    }

    let changed_files = git::changed_files(&repo, remote_name)?;
    let touched_by = |entry: &Entry| {
        changed_files
            .iter()
            .any(|file| Path::new(file).starts_with(&entry.repo_path))
    };
    if !changed_files.iter().all(|file| {
        auto_apply
            .iter()
            .any(|entry| Path::new(file).starts_with(&entry.repo_path))
    }) {
        return ask_for_manual_pull(" (some changes are outside auto_apply)");
    }

    // Decide before pulling: afterwards the repository no longer shows the local state
    let to_apply: Vec<&Entry> = auto_apply
        .into_iter()
        .filter(|entry| touched_by(entry))
        .collect();
    let locally_changed: Vec<&str> = to_apply
        .iter()
        .filter(|entry| {
            let source = entry.expanded_source();
            source.exists()
                && sync::has_changes(&source, &entry.full_repo_path(repo_root), ignore)
                    .unwrap_or(true)
        })
        .map(|entry| entry.source.as_str())
        .collect();
    if !locally_changed.is_empty() {
        return ask_for_manual_pull(&format!(
            " ({} also changed here and is not pushed yet)",
            locally_changed.join(", ")
        ));
    }

    if let Err(error) = git::pull(&repo, remote_name) {
        if first_notice {
            send_notification("dots", &format!("Pull failed: {}", error), None);
        }
        return Err(error);
    }
    println!(
        "Pulled {} commit{} from {}",
        count,
        if count == 1 { "" } else { "s" },
        remote_name
    );

    let mut failed = Vec::new();
    for entry in to_apply {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(repo_root);
        let result = if repo_path.exists() {
            sync::copy_entry(&repo_path, &source, ignore)
        } else {
            Ok(())
        };
        match result {
            Ok(()) => send_notification("dots", &format!("Applied {}", entry.source), None),
            Err(error) => {
                send_notification(
                    "dots",
                    &format!(
                        "Could not apply {}, run `dots link`: {:#}",
                        entry.source, error
                    ),
                    Some("link"),
                );
                failed.push(source);
            }
        }
    }

    Ok(failed)
}

/// Send a desktop notification. With `click_command`, a dots subcommand such
/// as "pull", clicking the notification opens a terminal running it.
fn send_notification(summary: &str, body: &str, click_command: Option<&'static str>) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut notification = notify_rust::Notification::new();
        notification.summary(summary).body(body).timeout(30000);
        if click_command.is_some() {
            notification.action("default", "Open");
        }
        if let (Ok(handle), Some(command)) = (notification.show(), click_command) {
            std::thread::spawn(move || {
                handle.wait_for_action(|action| {
                    if action == "default" {
                        open_terminal(command);
                    }
                })
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        let quote = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
        // Clicking a plain notification only opens Script Editor, so a clickable one is a dialog
        let script = match click_command {
            Some(command) => format!(
                "if button returned of (display dialog {} with title {} buttons {{\"Later\", \"Open dots {}\"}} default button 2 giving up after 3600) is \"Open dots {}\" then\n\
                 tell application \"Terminal\" to do script (quoted form of {}) & \" {}\"\n\
                 tell application \"Terminal\" to activate\n\
                 end if",
                quote(body),
                quote(summary),
                command,
                command,
                quote(&dots_executable()),
                command
            ),
            None => format!(
                "display notification {} with title {}",
                quote(body),
                quote(summary)
            ),
        };
        // A dialog waits for an answer; the watcher must keep running meanwhile
        std::thread::spawn(move || {
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output();
        });
    }

    let _ = click_command;
    println!("[notify] {}: {}", summary, body);
}

fn dots_executable() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "dots".to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_terminal(command: &str) {
    let script = format!(
        "'{}' {}; printf '\\nPress Enter to close '; read _",
        dots_executable(),
        command
    );
    match std::process::Command::new("xdg-terminal-exec")
        .args(["sh", "-c", &script])
        .spawn()
    {
        Ok(mut terminal) => {
            let _ = terminal.wait();
        }
        Err(error) => eprintln!(
            "Could not open a terminal with xdg-terminal-exec: {}",
            error
        ),
    }
}
