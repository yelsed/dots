use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::config::DotsConfig;
use crate::platform::Platform;

pub fn run() -> Result<()> {
    let repo_root = DotsConfig::find_repo_root()?;
    let toml_path = repo_root.join("dots.toml");

    if !toml_path.exists() {
        bail!("dots.toml not found at {}. Run 'dots init' first.", toml_path.display());
    }

    let path_str = toml_path.display().to_string();

    let mut cmd = match Platform::current() {
        Platform::Macos => {
            let mut c = Command::new("open");
            c.arg(&path_str);
            c
        }
        Platform::Linux => {
            let mut c = Command::new("xdg-open");
            c.arg(&path_str);
            c
        }
        Platform::Windows => {
            let mut c = Command::new("cmd");
            c.args(["/c", "start", "", &path_str]);
            c
        }
    };

    cmd.spawn().context("Failed to open dots.toml")?;

    Ok(())
}
