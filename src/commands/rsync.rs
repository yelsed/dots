use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{expand_tilde, DotsConfig, RsyncConfig};

pub fn run(
    path: Option<String>,
    host: Option<String>,
    dest: Option<String>,
    dry_run: bool,
) -> Result<()> {
    // Check rsync is available
    Command::new("rsync")
        .arg("--version")
        .output()
        .context("rsync is not installed or not in PATH")?;

    // Load config (may not exist — all resolution falls back to CLI flags then)
    let loaded = DotsConfig::load_default().ok();
    let rsync_cfg = loaded.as_ref().map(|(cfg, _)| cfg.rsync.clone());
    let repo_root = loaded.as_ref().map(|(_, root)| root.clone());

    // Resolve the local source folder
    let local_path = resolve_source(path, rsync_cfg.as_ref(), repo_root.as_deref())?;
    if !local_path.exists() {
        bail!(
            "Source folder does not exist: {}\n\
             Create it, or set [rsync] source = \"...\" in dots.toml",
            local_path.display()
        );
    }

    // Resolve host and dest
    let (resolved_host, resolved_dest) = resolve_host_dest(host, dest, rsync_cfg.as_ref())?;

    // Build the rsync command
    let remote = format!("{}:{}/", resolved_host, resolved_dest);
    let local_str = local_path.display().to_string();

    let mut cmd = Command::new("rsync");
    cmd.args(["-avz", "--progress"]);
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.arg(&local_str);
    cmd.arg(&remote);

    // Show what we're about to run
    let flag_str = if dry_run {
        "-avz --progress --dry-run"
    } else {
        "-avz --progress"
    };
    println!(
        "{} rsync {} {} {}",
        "→".blue().bold(),
        flag_str,
        local_str,
        remote
    );
    println!();

    // Run it
    let status = cmd.status().context("Failed to execute rsync")?;

    if !status.success() {
        bail!("rsync exited with status {}", status);
    }

    println!();
    println!("{} Done.", "✓".green().bold());
    Ok(())
}

/// Resolve the local source folder.
/// Precedence: CLI arg → [rsync].source in dots.toml → <repo_root>/rsync
fn resolve_source(
    path_arg: Option<String>,
    rsync_cfg: Option<&RsyncConfig>,
    repo_root: Option<&std::path::Path>,
) -> Result<PathBuf> {
    if let Some(p) = path_arg {
        return Ok(expand_tilde(&p));
    }

    if let Some(cfg) = rsync_cfg {
        if !cfg.source.is_empty() {
            return Ok(expand_tilde(&cfg.source));
        }
    }

    if let Some(root) = repo_root {
        return Ok(root.join("rsync"));
    }

    bail!(
        "No source folder configured. Pass a path argument, \
         set [rsync] source = \"...\" in dots.toml, or run inside a dots repo."
    )
}

fn resolve_host_dest(
    host_arg: Option<String>,
    dest_arg: Option<String>,
    rsync_cfg: Option<&RsyncConfig>,
) -> Result<(String, String)> {
    let cfg_host = rsync_cfg.and_then(|r| {
        if r.host.is_empty() {
            None
        } else {
            Some(r.host.clone())
        }
    });
    let cfg_dest = rsync_cfg.and_then(|r| {
        if r.dest.is_empty() {
            None
        } else {
            Some(r.dest.clone())
        }
    });

    let host = host_arg.or(cfg_host).ok_or_else(|| {
        anyhow::anyhow!(
            "No host configured. Use {} or add {} to dots.toml",
            "--host".bold(),
            "[rsync] host = \"...\"".bold()
        )
    })?;

    let dest = dest_arg.or(cfg_dest).ok_or_else(|| {
        anyhow::anyhow!(
            "No destination configured. Use {} or add {} to dots.toml",
            "--dest".bold(),
            "[rsync] dest = \"...\"".bold()
        )
    })?;

    Ok((host, dest))
}
