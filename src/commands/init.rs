use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

use crate::config::{DotsConfig, Entry, RepoConfig, RsyncConfig, WatchConfig};
use crate::git;
use crate::platform::Platform;

pub fn run(remote: Option<String>, path: Option<String>) -> Result<()> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let repo_path = path
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("dotfiles"));

    println!("{}", "dots init".bold());

    if let Some(remote_url) = remote {
        if repo_path.exists() {
            anyhow::bail!(
                "Directory already exists: {}. Remove it first or use --path to specify a different location.",
                repo_path.display()
            );
        }

        println!("Cloning {} -> {}", remote_url.cyan(), repo_path.display());
        git::clone_repo(&remote_url, &repo_path)?;
        println!("{}", "Cloned successfully.".green());
    } else if !repo_path.exists() {
        println!("Creating new dotfiles repo at {}", repo_path.display());
        std::fs::create_dir_all(&repo_path)?;
        git2::Repository::init(&repo_path)?;

        // Create directory structure
        for dir in &["shared", "linux", "macos", "windows"] {
            std::fs::create_dir_all(repo_path.join(dir))?;
        }
        println!("{}", "Initialized new repo.".green());
    } else {
        println!("Using existing repo at {}", repo_path.display().to_string().cyan());
    }

    // Create dots.toml if it doesn't exist
    let toml_path = repo_path.join("dots.toml");
    if !toml_path.exists() {
        println!("Creating dots.toml...");
        let config = DotsConfig {
            repo: RepoConfig {
                remote: "origin".to_string(),
            },
            watch: WatchConfig { debounce_secs: 3 },
            entry: default_entries(),
            rsync: Some(RsyncConfig::default()),
        };
        config.save(&toml_path)?;
        println!("{}", "Created dots.toml with default entries.".green());
    }

    // Create the default rsync drop folder inside the repo
    let rsync_dir = repo_path.join("rsync");
    if !rsync_dir.exists() {
        std::fs::create_dir_all(&rsync_dir)?;
        println!("Created rsync drop folder at {}", rsync_dir.display());
    }

    // Run link
    println!("\n{}", "Linking configs...".bold());
    let config = DotsConfig::load(&toml_path)?;
    let entries = config.platform_entries();

    for entry in &entries {
        let repo_file = entry.full_repo_path(&repo_path);
        let source = entry.expanded_source();

        if repo_file.exists() && !source.exists() {
            if let Some(parent) = source.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::sync::copy_entry(&repo_file, &source)?;
            println!("  {} {}", "link".green(), entry.source);
        } else if repo_file.exists() && source.exists() {
            println!("  {} {} (already exists)", "ok".green(), entry.source);
        } else {
            println!("  {} {} (not in repo)", "skip".yellow(), entry.source);
        }
    }

    println!("\n{} Run {} to start auto-syncing.", "Done!".green().bold(), "dots watch".cyan());
    Ok(())
}

fn default_entries() -> Vec<Entry> {
    let current = Platform::current();
    let mut entries = vec![
        Entry {
            source: "~/.config/nvim".to_string(),
            repo_path: "shared/nvim".to_string(),
            platforms: vec![Platform::Linux, Platform::Macos],
        },
        Entry {
            source: "~/.config/ghostty".to_string(),
            repo_path: "shared/ghostty".to_string(),
            platforms: vec![Platform::Linux, Platform::Macos],
        },
        Entry {
            source: "~/.bashrc".to_string(),
            repo_path: "shared/shell/.bashrc".to_string(),
            platforms: vec![Platform::Linux, Platform::Macos],
        },
    ];

    if current == Platform::Linux {
        entries.push(Entry {
            source: "~/.config/hypr/bindings.conf".to_string(),
            repo_path: "linux/hypr/bindings.conf".to_string(),
            platforms: vec![Platform::Linux],
        });
        entries.push(Entry {
            source: "~/.config/hypr/monitors.conf".to_string(),
            repo_path: "linux/hypr/monitors.conf".to_string(),
            platforms: vec![Platform::Linux],
        });
    }

    if current == Platform::Macos {
        entries.push(Entry {
            source: "~/.config/zed".to_string(),
            repo_path: "shared/zed".to_string(),
            platforms: vec![Platform::Linux, Platform::Macos],
        });
    }

    entries
}
