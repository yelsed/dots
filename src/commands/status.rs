use anyhow::Result;
use colored::Colorize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::DotsConfig;
use crate::git;
use crate::platform::Platform;
use crate::sync::{self, ChangeStatus};

pub fn run(no_fetch: bool, verbose: bool) -> Result<()> {
    let (config, repo_root) = DotsConfig::load_default()?;
    let current = Platform::current();

    println!("{}", "dots status".bold());
    println!("Platform: {}", current.to_string().cyan());
    println!("Repo:     {}", repo_root.display().to_string().cyan());
    println!();

    let all = config.all_entries();
    let changes = sync::diff_summary(&all, &repo_root)?;

    let relevant = config.platform_entries();
    let relevant_paths: Vec<_> = relevant.iter().map(|e| e.repo_path.as_str()).collect();

    // Opening the repo is best-effort: mtime hints still work without it,
    // we just lose git-commit-time lookups for repo-only / synced entries.
    let repo = git::open_repo(&repo_root).ok();

    for change in &changes {
        let is_relevant = relevant_paths.contains(&change.entry.repo_path.as_str());
        let platforms: Vec<_> = change.entry.platforms.iter().map(|p| p.to_string()).collect();
        let platform_str = platforms.join(",");

        let status_str = match &change.status {
            ChangeStatus::Synced => "synced".green().to_string(),
            ChangeStatus::Modified => "modified".yellow().to_string(),
            ChangeStatus::SystemOnly => "not in repo".blue().to_string(),
            ChangeStatus::RepoOnly => "not linked".magenta().to_string(),
            ChangeStatus::Missing => "missing".red().to_string(),
        };

        let relevance = if is_relevant {
            ""
        } else {
            " (not this platform)"
        };

        // Pick the most meaningful timestamp per status.
        //   - Modified / SystemOnly → system file mtime (when the user
        //     touched the live file).
        //   - RepoOnly / Synced → last git commit that touched the repo
        //     path (surviving checkouts, unlike filesystem mtime). Falls
        //     back to filesystem mtime if the git lookup yields nothing
        //     (e.g. newly `dots add`ed and not yet committed).
        //   - Missing → no timestamp.
        let time_str = {
            let source = change.entry.expanded_source();
            match change.status {
                ChangeStatus::Modified | ChangeStatus::SystemOnly => {
                    sync::last_modified(&source).ok().flatten().map(relative_time)
                }
                ChangeStatus::RepoOnly => repo
                    .as_ref()
                    .and_then(|r| {
                        git::last_commit_time_for_path(r, &change.entry.repo_path)
                            .ok()
                            .flatten()
                    })
                    .map(relative_time_from_unix)
                    .or_else(|| {
                        sync::last_modified(&change.entry.full_repo_path(&repo_root))
                            .ok()
                            .flatten()
                            .map(relative_time)
                    }),
                ChangeStatus::Synced => repo
                    .as_ref()
                    .and_then(|r| {
                        git::last_commit_time_for_path(r, &change.entry.repo_path)
                            .ok()
                            .flatten()
                    })
                    .map(relative_time_from_unix)
                    .or_else(|| sync::last_modified(&source).ok().flatten().map(relative_time)),
                ChangeStatus::Missing => None,
            }
        };
        let time_col = format!("{:>10}", time_str.unwrap_or_default());
        // Paint the timestamp in the status color for repo-only entries so
        // they visually pop out from the rest of the list.
        let time_col_colored = match change.status {
            ChangeStatus::RepoOnly => time_col.magenta().to_string(),
            _ => time_col.dimmed().to_string(),
        };

        println!(
            "  {} {} {} [{}]{}",
            time_col_colored,
            status_str,
            change.entry.source.dimmed(),
            platform_str.dimmed(),
            relevance.dimmed(),
        );

        // Show detail sub-line for modified directory entries
        if let Some(detail) = &change.detail {
            let mut parts = Vec::new();
            if detail.content_changed > 0 {
                parts.push(format!(
                    "{} file{} changed",
                    detail.content_changed,
                    if detail.content_changed == 1 { "" } else { "s" }
                ));
            }
            if detail.local_only > 0 {
                parts.push(format!(
                    "{} file{} only locally",
                    detail.local_only,
                    if detail.local_only == 1 { "" } else { "s" }
                ));
            }
            if detail.repo_only > 0 {
                parts.push(format!(
                    "{} file{} only in repo",
                    detail.repo_only,
                    if detail.repo_only == 1 { "" } else { "s" }
                ));
            }
            if !parts.is_empty() {
                println!(
                    "             {} {}",
                    "↳".dimmed(),
                    parts.join(", ").yellow()
                );
                if verbose {
                    for f in &detail.changed_files {
                        println!("               {} {}", "~".yellow(), f.display().to_string().dimmed());
                    }
                    for f in &detail.local_only_files {
                        println!("               {} {}", "+".blue(), f.display().to_string().dimmed());
                    }
                    for f in &detail.repo_only_files {
                        println!("               {} {}", "-".magenta(), f.display().to_string().dimmed());
                    }
                }
            }
        }
    }

    let modified_count = changes
        .iter()
        .filter(|c| c.status == ChangeStatus::Modified)
        .count();
    let synced_count = changes
        .iter()
        .filter(|c| c.status == ChangeStatus::Synced)
        .count();

    println!();
    println!(
        "{} entries tracked, {} synced, {} modified",
        changes.len().to_string().bold(),
        synced_count.to_string().green(),
        modified_count.to_string().yellow()
    );

    // Remote sync state
    println!();
    print_remote_status(&config, repo.as_ref(), no_fetch);

    Ok(())
}

fn print_remote_status(config: &DotsConfig, repo: Option<&git2::Repository>, no_fetch: bool) {
    if no_fetch {
        println!("Remote: {}", "(skipped, --no-fetch)".dimmed());
        return;
    }

    let repo = match repo {
        Some(r) => r,
        None => {
            println!("Remote: {}", "unavailable (could not open repo)".dimmed().red());
            return;
        }
    };

    let remote_name = &config.repo.remote;
    let status = match git::fetch_and_check(repo, remote_name) {
        Ok(s) => s,
        Err(e) => {
            println!("Remote: {}", format!("unavailable ({})", e).dimmed().red());
            return;
        }
    };

    match status {
        git::RemoteStatus::UpToDate => {
            println!("Remote: {}", "up to date".green());
        }
        git::RemoteStatus::Behind(n) => {
            let when = match git::remote_head_time(repo, remote_name) {
                Ok(secs) => format!(" — latest {}", relative_time_from_unix(secs)),
                Err(_) => String::new(),
            };
            println!(
                "Remote: {}{}",
                format!("{} commit(s) behind", n).yellow(),
                when.dimmed()
            );
            if let Ok(files) = git::changed_files(repo, remote_name) {
                for f in files {
                    println!("  {}", f.dimmed());
                }
            }
            println!("  {}", "run `dots pull` to apply".dimmed());
        }
        git::RemoteStatus::Ahead(n) => {
            println!("Remote: {}", format!("{} commit(s) ahead", n).blue());
            println!("  {}", "run `dots push` to publish".dimmed());
        }
    }
}

/// Format a SystemTime as a relative "N units ago" string.
fn relative_time(t: SystemTime) -> String {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => relative_time_from_unix(d.as_secs() as i64),
        Err(_) => "unknown".to_string(),
    }
}

/// Format a unix timestamp (seconds) as a relative "N units ago" string.
fn relative_time_from_unix(secs: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = now - secs;
    if delta < 0 {
        return "in the future".to_string();
    }
    if delta < 5 {
        return "just now".to_string();
    }
    if delta < 60 {
        return format!("{}s ago", delta);
    }
    if delta < 3600 {
        return format!("{}m ago", delta / 60);
    }
    if delta < 86_400 {
        return format!("{}h ago", delta / 3600);
    }
    if delta < 30 * 86_400 {
        return format!("{}d ago", delta / 86_400);
    }
    if delta < 365 * 86_400 {
        return format!("{}mo ago", delta / (30 * 86_400));
    }
    format!("{}y ago", delta / (365 * 86_400))
}
