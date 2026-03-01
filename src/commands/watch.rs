use anyhow::Result;
use colored::Colorize;

use crate::config::DotsConfig;
use crate::watcher;

pub fn run(poll_interval: u64) -> Result<()> {
    let (config, repo_root) = DotsConfig::load_default()?;

    println!("{}", "dots watch".bold());
    println!("Debounce:      {} seconds", config.watch.debounce_secs);
    println!("Remote poll:   every {} minutes", poll_interval);
    println!("Press {} to stop", "Ctrl+C".yellow());
    println!();

    watcher::run_watcher(&config, &repo_root, poll_interval)
}
