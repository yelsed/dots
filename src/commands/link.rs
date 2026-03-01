use anyhow::Result;
use colored::Colorize;

use crate::config::DotsConfig;
use crate::sync;

pub fn run(force: bool) -> Result<()> {
    let (config, repo_root) = DotsConfig::load_default()?;
    let entries = config.platform_entries();

    if entries.is_empty() {
        println!("{}", "No entries for this platform.".yellow());
        return Ok(());
    }

    println!("{}", "Linking configs from repo to system...".bold());

    for entry in &entries {
        let source = entry.expanded_source();
        let repo_path = entry.full_repo_path(&repo_root);

        if !repo_path.exists() {
            println!("  {} {} (not in repo yet)", "skip".yellow(), entry.repo_path);
            continue;
        }

        if source.exists() && !force {
            // Check if they differ
            if sync::has_changes(&repo_path, &source)? {
                println!(
                    "  {} {} (exists, differs — use --force to overwrite)",
                    "skip".yellow(),
                    entry.source
                );
            } else {
                println!("  {} {} (already synced)", "ok".green(), entry.source);
            }
            continue;
        }

        // Create parent dirs if needed
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
