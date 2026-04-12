use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::MultiSelect;
use std::path::PathBuf;

use crate::config::{expand_tilde, DotsConfig};
use crate::platform::Platform;
use crate::sync;

use super::add::determine_repo_path;

struct ScanPreset {
    agent_name: &'static str,
    paths: Vec<&'static str>,
}

fn build_presets() -> Vec<ScanPreset> {
    vec![
        ScanPreset {
            agent_name: "Claude Code",
            paths: vec![
                "~/.claude/agents",
                "~/.claude/commands",
                "~/.claude/settings.json",
                "~/.agents/skills",
            ],
        },
        // Cursor excluded — ~/.cursor/ contains extensions and caches (2GB+)
        // TODO: add back with targeted paths once Cursor supports a clean config dir
        ScanPreset {
            agent_name: "GitHub Copilot",
            paths: vec!["~/.config/github-copilot"],
        },
        ScanPreset {
            agent_name: "Codex",
            paths: vec!["~/.codex"],
        },
        ScanPreset {
            agent_name: "Gemini",
            paths: vec!["~/.gemini"],
        },
        ScanPreset {
            agent_name: "Continue.dev",
            paths: vec!["~/.continue"],
        },
    ]
}

struct Candidate {
    agent_name: &'static str,
    source_path: PathBuf,
    source_display: String,
}

pub fn run(target: Option<String>, platforms_arg: Option<String>) -> Result<()> {
    let (mut config, repo_root) = DotsConfig::load_default()?;

    let presets = build_presets();

    // Filter by target if specified
    let presets: Vec<&ScanPreset> = if let Some(ref t) = target {
        let t_lower = t.to_lowercase();
        let filtered: Vec<_> = presets
            .iter()
            .filter(|p| p.agent_name.to_lowercase().contains(&t_lower))
            .collect();
        if filtered.is_empty() {
            let names: Vec<_> = presets.iter().map(|p| p.agent_name).collect();
            anyhow::bail!(
                "Unknown target '{}'. Available: {}",
                t,
                names.join(", ")
            );
        }
        filtered
    } else {
        presets.iter().collect()
    };

    // Discover candidates
    let mut candidates: Vec<Candidate> = Vec::new();

    for preset in &presets {
        for path_str in &preset.paths {
            let expanded = expand_tilde(path_str);

            if !expanded.exists() {
                continue;
            }

            // Skip if already tracked
            if config.is_tracked(&expanded) {
                continue;
            }

            candidates.push(Candidate {
                agent_name: preset.agent_name,
                source_path: expanded,
                source_display: path_str.to_string(),
            });
        }
    }

    if candidates.is_empty() {
        println!(
            "{}",
            "Nothing new to add — all discovered paths are already tracked or don't exist."
                .yellow()
        );
        return Ok(());
    }

    // Build display labels for multi-select
    let labels: Vec<String> = candidates
        .iter()
        .map(|c| format!("[{}] {}", c.agent_name, c.source_display))
        .collect();

    println!("{}", "Discovered AI agent configs:".bold());

    let selections = MultiSelect::new()
        .with_prompt("Select entries to add (space to toggle, enter to confirm)")
        .items(&labels)
        .defaults(&vec![true; labels.len()])
        .interact()?;

    if selections.is_empty() {
        println!("{}", "Nothing selected.".yellow());
        return Ok(());
    }

    // Determine platforms
    let platforms = if let Some(p) = platforms_arg {
        p.split(',')
            .filter_map(|s| Platform::from_str(s.trim()))
            .collect::<Vec<_>>()
    } else {
        // Default to all platforms for agent configs
        vec![Platform::Linux, Platform::Macos, Platform::Windows]
    };

    let mut added = 0;

    for &idx in &selections {
        let candidate = &candidates[idx];
        let repo_path = determine_repo_path(&candidate.source_path, &platforms)?;
        let full_repo_path = repo_root.join(&repo_path);

        sync::copy_entry(&candidate.source_path, &full_repo_path)
            .with_context(|| format!("Failed to copy {}", candidate.source_display))?;

        config.add_entry(&candidate.source_display, &repo_path, &platforms);

        println!(
            "  {} {} -> {}",
            "Added".green(),
            candidate.source_display.cyan(),
            repo_path.cyan()
        );
        added += 1;
    }

    config.save(&repo_root.join("dots.toml"))?;
    println!(
        "\n{} {} entries added to dots.toml",
        "Done!".green().bold(),
        added
    );

    Ok(())
}
