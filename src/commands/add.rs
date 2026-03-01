use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Select;
use std::path::PathBuf;

use crate::config::{contract_tilde, DotsConfig, Entry};
use crate::platform::Platform;
use crate::sync;

pub fn run(path: String, platforms_arg: Option<String>) -> Result<()> {
    let (mut config, repo_root) = DotsConfig::load_default()?;

    // Resolve the source path
    let source_path = if path.starts_with('~') {
        crate::config::expand_tilde(&path)
    } else {
        let p = PathBuf::from(&path);
        if p.is_absolute() {
            p
        } else {
            std::env::current_dir()?.join(&path)
        }
    };

    let source_path = source_path
        .canonicalize()
        .with_context(|| format!("Path does not exist: {}", path))?;

    let source_str = contract_tilde(&source_path);

    // Check if already tracked
    if config.entry.iter().any(|e| e.expanded_source() == source_path) {
        anyhow::bail!("{} is already tracked", source_str);
    }

    // Determine platforms
    let platforms = if let Some(p) = platforms_arg {
        p.split(',')
            .filter_map(|s| Platform::from_str(s.trim()))
            .collect::<Vec<_>>()
    } else {
        // Interactive selection
        let options = vec!["shared (linux + macos)", "linux only", "macos only", "all platforms"];
        let selection = Select::new()
            .with_prompt("Which platforms?")
            .items(&options)
            .default(0)
            .interact()?;

        match selection {
            0 => vec![Platform::Linux, Platform::Macos],
            1 => vec![Platform::Linux],
            2 => vec![Platform::Macos],
            3 => vec![Platform::Linux, Platform::Macos, Platform::Windows],
            _ => vec![Platform::Linux, Platform::Macos],
        }
    };

    // Determine repo path
    let repo_path = determine_repo_path(&source_path, &platforms)?;

    println!("{}", "Adding entry:".bold());
    println!("  Source:    {}", source_str.cyan());
    println!("  Repo path: {}", repo_path.cyan());
    let platform_strs: Vec<_> = platforms.iter().map(|p| p.to_string()).collect();
    println!("  Platforms: {}", platform_strs.join(", ").cyan());

    // Copy to repo
    let full_repo_path = repo_root.join(&repo_path);
    sync::copy_entry(&source_path, &full_repo_path)?;
    println!("  {} Copied to repo", "->".green());

    // Add entry to config
    config.entry.push(Entry {
        source: source_str,
        repo_path,
        platforms,
    });

    config.save(&repo_root.join("dots.toml"))?;
    println!("{}", "Entry added to dots.toml".green());

    Ok(())
}

/// Determine where in the repo structure a path should go
fn determine_repo_path(source_path: &PathBuf, platforms: &[Platform]) -> Result<String> {
    let home = dirs::home_dir().context("No home directory")?;
    let config_dir = home.join(".config");

    // Determine the category prefix
    let prefix = if platforms.len() == 1 {
        match &platforms[0] {
            Platform::Linux => "linux",
            Platform::Macos => "macos",
            Platform::Windows => "windows",
        }
    } else {
        "shared"
    };

    // Determine the relative path
    let relative = if let Ok(rel) = source_path.strip_prefix(&config_dir) {
        rel.display().to_string()
    } else if let Ok(rel) = source_path.strip_prefix(&home) {
        rel.display().to_string()
    } else {
        source_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    Ok(format!("{}/{}", prefix, relative))
}
