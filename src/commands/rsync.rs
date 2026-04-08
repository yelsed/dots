use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::Path;
use std::process::Command;

use crate::config::DotsConfig;

pub fn run(path: String, host: Option<String>, dest: Option<String>, dry_run: bool) -> Result<()> {
    // Check rsync is available
    Command::new("rsync")
        .arg("--version")
        .output()
        .context("rsync is not installed or not in PATH")?;

    // Validate local path
    let local_path = Path::new(&path);
    if !local_path.exists() {
        bail!("Local path does not exist: {}", path);
    }

    // Resolve host and dest: CLI flags take priority, then dots.toml
    let (resolved_host, resolved_dest) = resolve_config(host, dest)?;

    // Build the rsync command
    let remote = format!("{}:{}/", resolved_host, resolved_dest);
    let mut cmd = Command::new("rsync");
    cmd.args(["-avz", "--progress"]);

    if dry_run {
        cmd.arg("--dry-run");
    }

    cmd.arg(&path);
    cmd.arg(&remote);

    // Show what we're about to run
    let flag_str = if dry_run { "-avz --progress --dry-run" } else { "-avz --progress" };
    println!(
        "{} rsync {} {} {}",
        "→".blue().bold(),
        flag_str,
        path,
        remote
    );
    println!();

    // Run it
    let status = cmd
        .status()
        .context("Failed to execute rsync")?;

    if !status.success() {
        bail!("rsync exited with status {}", status);
    }

    println!();
    println!("{} Done.", "✓".green().bold());
    Ok(())
}

fn resolve_config(
    host_arg: Option<String>,
    dest_arg: Option<String>,
) -> Result<(String, String)> {
    // Try loading dots.toml for defaults (ignore errors — config may not exist)
    let rsync_cfg = DotsConfig::load_default()
        .ok()
        .and_then(|(cfg, _)| cfg.rsync);

    let host = host_arg
        .or_else(|| rsync_cfg.as_ref().and_then(|r| r.host.clone()))
        .ok_or_else(|| anyhow::anyhow!(
            "No host configured. Use {} or add {} to dots.toml",
            "--host".bold(),
            "[rsync] host = \"...\"".bold()
        ))?;

    let dest = dest_arg
        .or_else(|| rsync_cfg.as_ref().and_then(|r| r.dest.clone()))
        .ok_or_else(|| anyhow::anyhow!(
            "No destination configured. Use {} or add {} to dots.toml",
            "--dest".bold(),
            "[rsync] dest = \"...\"".bold()
        ))?;

    Ok((host, dest))
}
