use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::platform::Platform;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RsyncConfig {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub dest: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct EntriesConfig {
    #[serde(default)]
    pub shared: BTreeMap<String, String>,
    #[serde(default)]
    pub linux: BTreeMap<String, String>,
    #[serde(default)]
    pub macos: BTreeMap<String, String>,
    #[serde(default)]
    pub windows: BTreeMap<String, String>,
}

impl EntriesConfig {
    pub fn is_empty(&self) -> bool {
        self.shared.is_empty()
            && self.linux.is_empty()
            && self.macos.is_empty()
            && self.windows.is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DotsConfig {
    pub repo: RepoConfig,
    pub watch: WatchConfig,
    #[serde(default)]
    pub entries: EntriesConfig,
    #[serde(default)]
    pub rsync: RsyncConfig,
    /// Deprecated: old [[entry]] format. Auto-migrated to `entries` on load.
    #[serde(default, skip_serializing)]
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
    pub source: String,
    pub repo_path: String,
    pub platforms: Vec<Platform>,
}

impl Entry {
    pub fn expanded_source(&self) -> PathBuf {
        expand_tilde(&self.source)
    }

    pub fn full_repo_path(&self, repo_root: &Path) -> PathBuf {
        repo_root.join(&self.repo_path)
    }
}

impl DotsConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
        let mut config: DotsConfig =
            toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;

        let mut needs_save = false;

        // Auto-migrate old [[entry]] format → new [entries.*] sections
        if !config.entry.is_empty() && config.entries.is_empty() {
            for e in std::mem::take(&mut config.entry) {
                config.add_entry(&e.source, &e.repo_path, &e.platforms);
            }
            needs_save = true;
            eprintln!("Migrated dots.toml to new platform-grouped format.");
        }

        // Backfill [rsync] block if the on-disk file never had one
        if !content.contains("[rsync]") {
            needs_save = true;
        }

        if needs_save {
            config.save(path)?;
        }

        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        std::fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Collect all entries across every section with computed platform lists.
    pub fn all_entries(&self) -> Vec<Entry> {
        let mut map: BTreeMap<String, (String, Vec<Platform>)> = BTreeMap::new();

        // Shared → all platforms
        for (source, repo_path) in &self.entries.shared {
            map.insert(
                source.clone(),
                (
                    repo_path.clone(),
                    vec![Platform::Linux, Platform::Macos, Platform::Windows],
                ),
            );
        }

        let sections = [
            (&self.entries.linux, Platform::Linux),
            (&self.entries.macos, Platform::Macos),
            (&self.entries.windows, Platform::Windows),
        ];

        for (section, platform) in &sections {
            for (source, repo_path) in *section {
                match map.get_mut(source) {
                    Some((_, platforms)) => {
                        if !platforms.contains(platform) {
                            platforms.push(platform.clone());
                        }
                    }
                    None => {
                        map.insert(source.clone(), (repo_path.clone(), vec![platform.clone()]));
                    }
                }
            }
        }

        map.into_iter()
            .map(|(source, (repo_path, platforms))| Entry {
                source,
                repo_path,
                platforms,
            })
            .collect()
    }

    /// Get entries relevant to the current platform (shared + platform-specific).
    pub fn platform_entries(&self) -> Vec<Entry> {
        self.all_entries()
            .into_iter()
            .filter(|e| e.platforms.contains(&Platform::current()))
            .collect()
    }

    /// Add an entry to the appropriate section(s) based on platforms.
    pub fn add_entry(&mut self, source: &str, repo_path: &str, platforms: &[Platform]) {
        let all = [Platform::Linux, Platform::Macos, Platform::Windows];
        if all.iter().all(|p| platforms.contains(p)) {
            self.entries
                .shared
                .insert(source.to_string(), repo_path.to_string());
        } else {
            for platform in platforms {
                let section = match platform {
                    Platform::Linux => &mut self.entries.linux,
                    Platform::Macos => &mut self.entries.macos,
                    Platform::Windows => &mut self.entries.windows,
                };
                section.insert(source.to_string(), repo_path.to_string());
            }
        }
    }

    /// Check if a source path is already tracked in any section.
    pub fn is_tracked(&self, source_path: &Path) -> bool {
        self.all_entries()
            .iter()
            .any(|e| e.expanded_source() == source_path)
    }

    pub fn find_repo_root() -> Result<PathBuf> {
        if let Ok(path) = std::env::var("DOTS_REPO") {
            let p = PathBuf::from(path);
            if p.join("dots.toml").exists() {
                return Ok(p);
            }
        }

        let home = dirs::home_dir().context("Could not determine home directory")?;
        let dotfiles = home.join("dotfiles");
        if dotfiles.join("dots.toml").exists() {
            return Ok(dotfiles);
        }

        anyhow::bail!(
            "Could not find dots.toml. Set DOTS_REPO or run 'dots init' in ~/dotfiles"
        )
    }

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

pub fn contract_tilde(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }
    }
    path.display().to_string()
}
