use anyhow::Result;
use colored::Colorize;
use dialoguer::MultiSelect;

use crate::config::DotsConfig;
use crate::git;
use crate::platform::{self, Platform};
use crate::sync;

pub fn run() -> Result<()> {
    let (config, repo_root) = DotsConfig::load_default()?;
    let repo = git::open_repo(&repo_root)?;

    println!("{}", "Fetching remote changes...".bold());
    let status = git::fetch_and_check(&repo, &config.repo.remote)?;

    match &status {
        git::RemoteStatus::UpToDate => {
            println!("{}", "Already up to date.".green());
        }
        git::RemoteStatus::Behind(n) => {
            println!("Remote is {} commit{} ahead.", n.to_string().yellow(), if *n == 1 { "" } else { "s" });
        }
        git::RemoteStatus::Ahead(n) => {
            println!("Local is {} commit{} ahead of remote.", n.to_string().cyan(), if *n == 1 { "" } else { "s" });
            println!("{}", "Nothing to pull.".green());
            return Ok(());
        }
    }

    if matches!(status, git::RemoteStatus::UpToDate) {
        return Ok(());
    }

    // Get list of changed files before pulling
    let changed = git::changed_files(&repo, &config.repo.remote)?;

    if changed.is_empty() {
        println!("{}", "No file changes detected.".green());
        git::pull(&repo, &config.repo.remote)?;
        return Ok(());
    }

    println!("\n{}", "Changed files:".bold());

    // Map changed files to entries
    let current = Platform::current();
    let all_entries = config.all_entries();
    let mut applicable = Vec::new();
    let mut skipped = Vec::new();

    for file in &changed {
        // Find matching entry
        let matching_entry = all_entries.iter().find(|e| {
            file.starts_with(&e.repo_path)
        });

        if let Some(entry) = matching_entry {
            if platform::is_relevant(&entry.platforms) {
                applicable.push((file.clone(), entry.clone()));
            } else {
                skipped.push((file.clone(), entry.clone()));
            }
        } else {
            // File not tracked by any entry, still show it
            applicable.push((file.clone(), crate::config::Entry {
                source: String::new(),
                repo_path: file.clone(),
                platforms: vec![current.clone()],
            }));
        }
    }

    // Show skipped (wrong platform)
    for (file, entry) in &skipped {
        let platforms: Vec<_> = entry.platforms.iter().map(|p| p.to_string()).collect();
        println!(
            "  {} {} [{}] (skipped — not this platform)",
            "~".dimmed(),
            file.dimmed(),
            platforms.join(",").dimmed()
        );
    }

    if applicable.is_empty() {
        println!("\n{}", "No applicable changes for this platform.".yellow());
        git::pull(&repo, &config.repo.remote)?;
        return Ok(());
    }

    // Show applicable and let user select
    let items: Vec<String> = applicable
        .iter()
        .map(|(file, entry)| {
            if entry.source.is_empty() {
                file.clone()
            } else {
                format!("{} -> {}", file, entry.source)
            }
        })
        .collect();

    println!();
    let selections = MultiSelect::new()
        .with_prompt("Select changes to apply (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&vec![true; items.len()])
        .interact()?;

    // Pull the changes into repo first
    git::pull(&repo, &config.repo.remote)?;
    println!("{}", "Pulled changes.".green());

    // Apply selected entries
    let mut applied = 0;
    for idx in selections {
        let (_, entry) = &applicable[idx];
        if entry.source.is_empty() {
            continue; // Untracked file in repo, nothing to apply
        }

        let repo_path = entry.full_repo_path(&repo_root);
        let source = entry.expanded_source();

        if repo_path.exists() {
            sync::copy_entry(&repo_path, &source)?;
            println!("  {} {}", "applied".green(), entry.source);
            applied += 1;
        }
    }

    println!("\n{} {} change{} applied.", "Done!".green().bold(), applied, if applied == 1 { "" } else { "s" });

    Ok(())
}
