use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::platform::Platform;

#[derive(Debug, Serialize, Deserialize)]
pub struct DotsConfig {
    pub repo: RepoConfig,
    pub watch: WatchConfig,
    #[serde(default)]
    pub entry: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default = "default_remote")]
    pub remote: String,
}

fn default_remote() -> String {
    "origin".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchConfig {
    #[serde(default = "default_debounce")]
    pub debounce_secs: u64,
}

fn default_debounce() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Source path on the system (e.g. "~/.config/nvim")
    pub source: String,
    /// Path within the dotfiles repo (e.g. "shared/nvim")
    pub repo_path: String,
    /// Which platforms this entry applies to
    pub platforms: Vec<Platform>,
}

impl Entry {
    /// Expand ~ in source path to actual home directory
    pub fn expanded_source(&self) -> PathBuf {
        expand_tilde(&self.source)
    }

    /// Get the full path within the dotfiles repo
    pub fn full_repo_path(&self, repo_root: &Path) -> PathBuf {
        repo_root.join(&self.repo_path)
    }
}

impl DotsConfig {
    /// Load config from a dots.toml file
    pub fn load(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
        let config: DotsConfig =
            toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(config)
    }

    /// Save config to a dots.toml file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        std::fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Get entries relevant to the current platform
    pub fn platform_entries(&self) -> Vec<&Entry> {
        let current = Platform::current();
        self.entry.iter().filter(|e| e.platforms.contains(&current)).collect()
    }

    /// Find the dotfiles repo root (where dots.toml lives)
    pub fn find_repo_root() -> Result<PathBuf> {
        // Check DOTS_REPO env var first
        if let Ok(path) = std::env::var("DOTS_REPO") {
            let p = PathBuf::from(path);
            if p.join("dots.toml").exists() {
                return Ok(p);
            }
        }

        // Default: ~/dotfiles
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let dotfiles = home.join("dotfiles");
        if dotfiles.join("dots.toml").exists() {
            return Ok(dotfiles);
        }

        anyhow::bail!(
            "Could not find dots.toml. Set DOTS_REPO or run 'dots init' in ~/dotfiles"
        )
    }

    /// Load config from the default repo location
    pub fn load_default() -> Result<(Self, PathBuf)> {
        let repo_root = Self::find_repo_root()?;
        let config = Self::load(&repo_root.join("dots.toml"))?;
        Ok((config, repo_root))
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

/// Contract home directory back to ~ for display/storage
pub fn contract_tilde(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }
    }
    path.display().to_string()
}
