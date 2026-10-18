# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

**dots** — a cross-platform dotfile synchronization tool written in Rust. It tracks config files across machines using a git-backed repository with platform-aware entries, change detection via SHA256 hashing, and both one-shot and continuous (watcher daemon) sync modes.

## Build & Run

```bash
cargo build                  # Debug build
cargo build --release        # Release build
cargo run -- <subcommand>    # Run via cargo (e.g., cargo run -- status)
```

No test suite exists yet. No linter or formatter configuration beyond default `cargo fmt` / `cargo clippy`.

## Architecture

The CLI (`main.rs`) uses clap derive macros to dispatch to subcommands in `src/commands/`:

- `init`: clone or create a dotfiles repo at `~/dotfiles`, scaffold platform dirs, generate `dots.toml`
- `add`: interactively track a file/directory (copies to repo, adds config entry)
- `push`: copy tracked files system→repo, git commit+push
- `pull`: git pull, interactively select entries to apply repo→system
- `watch`: background daemon with debounced file watching + periodic remote polling
- `status`: display sync state of all tracked entries
- `link`: copy files from repo to system locations (copies, never symlinks)
- `scan`: find known AI agent config locations and bulk-add them
- `rsync`: rsync a folder to a remote host over SSH, using the `[rsync]` block in `dots.toml`
- `update`: self-update from GitHub Releases
- `config`: open `dots.toml` in the editor

Core modules:

| Module | Purpose |
|---|---|
| `config.rs` | Load/save `dots.toml` (TOML); defines `DotsConfig`, `RepoConfig`, `WatchConfig`, `Entry` |
| `sync.rs` | SHA256-based change detection, file copying, recursive directory comparison. States: `Synced`, `Modified`, `SystemOnly`, `RepoOnly`, `Missing` |
| `git.rs` | Wraps `git2` for clone/commit/push/pull with SSH key + agent + credential fallback auth |
| `platform.rs` | Runtime platform detection via `cfg!()`, `Platform` enum (Linux/MacOS/Windows) |
| `watcher.rs` | Uses `notify` crate for filesystem events, debounce logic, remote poll timer |

## Key Conventions

- All fallible functions return `anyhow::Result<T>` with `.context()` for error messages
- Paths use tilde expansion/contraction (`~` ↔ `$HOME`) throughout config and display
- Config lives at `${DOTS_REPO:-$HOME/dotfiles}/dots.toml`
- `dots.toml` groups entries as `[entries.shared|linux|macos|windows]` maps of system path to repo path; `config.rs` flattens them into `Entry` values with a `platforms` list. The old `[[entry]]` format is auto-migrated on load
- `push` is additive: `copy_entry` never deletes repo files that are missing locally, and push does not fetch first
- Git auth tries: SSH keys (ed25519, rsa) → SSH agent → env vars (`GIT_USERNAME`/`GIT_PASSWORD`)
- Pulls are fast-forward-only for safety
