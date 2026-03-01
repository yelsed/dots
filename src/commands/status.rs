use anyhow::Result;
use colored::Colorize;

use crate::config::DotsConfig;
use crate::platform::Platform;
use crate::sync::{self, ChangeStatus};

pub fn run() -> Result<()> {
    let (config, repo_root) = DotsConfig::load_default()?;
    let current = Platform::current();

    println!("{}", "dots status".bold());
    println!("Platform: {}", current.to_string().cyan());
    println!("Repo:     {}", repo_root.display().to_string().cyan());
    println!();

    let entries: Vec<_> = config.entry.iter().collect();
    let changes = sync::diff_summary(&entries, &repo_root)?;

    let relevant = config.platform_entries();
    let relevant_paths: Vec<_> = relevant.iter().map(|e| &e.repo_path).collect();

    for change in &changes {
        let is_relevant = relevant_paths.contains(&&change.entry.repo_path);
        let platforms: Vec<_> = change.entry.platforms.iter().map(|p| p.to_string()).collect();
        let platform_str = platforms.join(",");

        let status_str = match &change.status {
            ChangeStatus::Synced => "synced".green().to_string(),
            ChangeStatus::Modified => "modified".yellow().to_string(),
            ChangeStatus::SystemOnly => "system only".blue().to_string(),
            ChangeStatus::RepoOnly => "repo only".magenta().to_string(),
            ChangeStatus::Missing => "missing".red().to_string(),
        };

        let relevance = if is_relevant {
            ""
        } else {
            " (not this platform)"
        };

        println!(
            "  {} {} [{}]{}",
            status_str,
            change.entry.source.dimmed(),
            platform_str.dimmed(),
            relevance.dimmed()
        );
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

    Ok(())
}
