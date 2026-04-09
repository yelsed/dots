use anyhow::Result;
use colored::Colorize;
use dialoguer::MultiSelect;

use crate::config::{DotsConfig, Entry};
use crate::sync;

pub fn run(force: bool) -> Result<()> {
    let (config, repo_root) = DotsConfig::load_default()?;
    let entries = config.platform_entries();

    if entries.is_empty() {
        println!("{}", "No entries for this platform.".yellow());
        return Ok(());
    }

    // Pass 1: scan only. No side effects, no linking yet. Classify every
    // entry so we can show the user the full picture before anything happens.
    let mut missing_in_repo: Vec<Entry> = Vec::new();
    let mut already_synced: Vec<Entry> = Vec::new();
    let mut new_links: Vec<Entry> = Vec::new(); // system file doesn't exist yet
    let mut conflicts: Vec<Entry> = Vec::new(); // system file exists and differs

    for entry in &entries {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(&repo_root);

        if !repo_path.exists() {
            missing_in_repo.push((*entry).clone());
            continue;
        }

        if !source.exists() {
            new_links.push((*entry).clone());
            continue;
        }

        if sync::has_changes(&repo_path, &source)? {
            conflicts.push((*entry).clone());
        } else {
            already_synced.push((*entry).clone());
        }
    }

    for e in &missing_in_repo {
        println!("  {} {} (not in repo yet)", "skip".yellow(), e.repo_path);
    }
    for e in &already_synced {
        println!("  {} {} (already synced)", "ok".green(), e.source);
    }

    if new_links.is_empty() && conflicts.is_empty() {
        println!();
        println!("{}", "Nothing to link.".green().bold());
        return Ok(());
    }

    // Pass 2: act. Build one multiselect with every actionable entry
    // pre-checked so the user can opt out of anything they want to keep.
    // With --force we bypass the prompt and link everything straight away.
    let mut actionable: Vec<Entry> = Vec::new();
    actionable.extend(new_links.iter().cloned());
    actionable.extend(conflicts.iter().cloned());

    let conflict_start = new_links.len();

    let to_link: Vec<&Entry> = if force {
        actionable.iter().collect()
    } else {
        let items: Vec<String> = actionable
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let tag = if i >= conflict_start { " (overwrite)" } else { "" };
                format!("{} <- {}{}", e.source, e.repo_path, tag)
            })
            .collect();

        println!();
        let selections = MultiSelect::new()
            .with_prompt("Select files to link (space to toggle, enter to confirm)")
            .items(&items)
            .defaults(&vec![true; items.len()])
            .interact()?;

        let selected: std::collections::HashSet<usize> = selections.into_iter().collect();
        actionable
            .iter()
            .enumerate()
            .filter_map(|(i, e)| if selected.contains(&i) { Some(e) } else { None })
            .collect()
    };

    println!();
    for entry in &actionable {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(&repo_root);

        if !to_link.iter().any(|e| e.source == entry.source) {
            println!("  {} {} (kept local)", "skip".yellow(), entry.source);
            continue;
        }

        if let Some(parent) = source.parent() {
            std::fs::create_dir_all(parent)?;
        }
        sync::copy_entry(&repo_path, &source)?;
        println!("  {} {} -> {}", "link".green(), entry.repo_path, entry.source);
    }

    println!();
    println!("{}", "Done!".green().bold());

    Ok(())
}
