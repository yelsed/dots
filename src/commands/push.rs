use anyhow::Result;
use colored::Colorize;

use crate::config::DotsConfig;
use crate::git;
use crate::sync::{self, ChangeStatus};

pub fn run(message: Option<String>) -> Result<()> {
    let (config, repo_root) = DotsConfig::load_default()?;
    let entries = config.platform_entries();

    println!("{}", "Scanning entries...".bold());
    let changes = sync::diff_summary(&entries, &repo_root)?;

    let modified: Vec<_> = changes
        .iter()
        .filter(|c| c.status == ChangeStatus::Modified || c.status == ChangeStatus::SystemOnly)
        .collect();

    if modified.is_empty() {
        println!("{}", "Everything is synced, nothing to push.".green());
        return Ok(());
    }

    println!("{}", "Copying changes to repo...".bold());

    let mut changed_files = Vec::new();
    for change in &modified {
        let source = change.entry.expanded_source();
        let repo_path = change.entry.full_repo_path(&repo_root);

        sync::copy_entry(&source, &repo_path)?;
        changed_files.push(change.entry.repo_path.clone());
        println!("  {} {}", "->".green(), change.entry.repo_path);
    }

    let commit_msg = message.unwrap_or_else(|| {
        if changed_files.len() == 1 {
            format!("sync: {}", changed_files[0])
        } else {
            format!(
                "sync: {}",
                changed_files.join(", ")
            )
        }
    });

    let repo = git::open_repo(&repo_root)?;
    let oid = git::commit_all(&repo, &commit_msg)?;
    println!("{} {}", "Committed:".green(), &oid.to_string()[..8]);

    match git::push(&repo, &config.repo.remote) {
        Ok(()) => println!("{} {}", "Pushed to".green(), config.repo.remote),
        Err(e) => {
            eprintln!(
                "{} Push failed: {}",
                "Warning:".yellow(),
                e
            );
            eprintln!("Changes are committed locally. Push manually with: git -C {} push", repo_root.display());
        }
    }

    Ok(())
}
