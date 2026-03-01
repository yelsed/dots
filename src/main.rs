mod commands;
mod config;
mod git;
mod platform;
mod sync;
mod watcher;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dots", about = "Cross-platform dotfile sync tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize: clone/setup dotfiles repo, create dots.toml, link configs
    Init {
        /// Git remote URL to clone
        remote: Option<String>,
        /// Path to dotfiles repo (default: ~/dotfiles)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Start tracking a config file or directory
    Add {
        /// Path to the config file or directory to track
        path: String,
        /// Platform(s) this config belongs to (comma-separated: linux,macos,windows)
        #[arg(short = 'P', long)]
        platforms: Option<String>,
    },
    /// Start file watcher — auto-commit+push on changes
    Watch {
        /// Poll interval for remote changes in minutes (default: 30)
        #[arg(long, default_value = "30")]
        poll_interval: u64,
    },
    /// Git pull, show diff, interactively choose what to apply
    Pull,
    /// One-shot: copy tracked changes to repo, commit, push
    Push {
        /// Commit message (default: auto-generated)
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Show tracked entries, what's changed, current platform
    Status,
    /// Copy all platform-relevant configs from repo to system
    Link {
        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { remote, path } => commands::init::run(remote, path),
        Commands::Add { path, platforms } => commands::add::run(path, platforms),
        Commands::Watch { poll_interval } => commands::watch::run(poll_interval),
        Commands::Pull => commands::pull::run(),
        Commands::Push { message } => commands::push::run(message),
        Commands::Status => commands::status::run(),
        Commands::Link { force } => commands::link::run(force),
    }
}
