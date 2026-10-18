# dots — Walkthrough

A cross-platform dotfile sync tool. Track your config files in a git repo and keep them in sync across all your machines.

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/yelsed/dots/master/install.sh | sh
```

Or from a clone of this repository:

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# binary is at target/release/dots
```

## Getting Started

### 1. Initialize your dotfiles repo

**New repo** (no existing dotfiles):

```bash
dots init
```

This creates `~/dotfiles/`, initializes a git repo, scaffolds platform directories (`shared/`, `linux/`, `macos/`, `windows/`), and generates a `dots.toml` config with some sensible defaults (nvim, ghostty, bashrc).

**Clone an existing repo:**

```bash
dots init git@github.com:you/dotfiles.git        # SSH — recommended
dots init https://github.com/you/dotfiles.git   # HTTPS — requires GIT_USERNAME/GIT_PASSWORD env vars
```

Clones the repo to `~/dotfiles/` and links all configs that match your current platform. SSH is recommended — it uses your local key or SSH agent with no extra setup. HTTPS requires a GitHub Personal Access Token set via env vars (see [Git authentication](#git-authentication)).

**Custom path:**

```bash
dots init --path ~/my-dots
dots init git@github.com:you/dotfiles.git --path ~/my-dots
```

### 2. Check what's tracked

```bash
dots status
```

Shows your current platform, the repo location, and the sync state of every tracked entry:

- **synced** — system file and repo copy are identical
- **modified** — they differ (you've made local changes)
- **system only** — file exists on your system but not yet copied to repo
- **repo only** — file is in the repo but not on this system
- **missing** — neither exists

Entries for other platforms are shown dimmed with "(not this platform)".

## Day-to-Day Usage

### Adding files to track

```bash
dots add ~/.config/starship.toml
```

You'll be prompted to pick platforms:
- shared (linux + macos)
- linux only
- macos only
- all platforms

Or specify directly:

```bash
dots add ~/.config/starship.toml -P linux,macos
```

The file is copied into the repo under the appropriate directory (e.g. `shared/starship.toml`) and an entry is added to `dots.toml`.

### Pushing changes

When you've edited a tracked config on your system:

```bash
dots push
```

This copies all modified files from your system into the repo, commits, and pushes to the remote. An auto-generated commit message lists which files changed.

Push only adds and overwrites files. A file you deleted locally stays in the repo until you remove it there with git. Push also does not pull first: when another machine pushed in the meantime, your commit stays local and the push fails. Run `git -C ~/dotfiles pull --ff-only`, then `git -C ~/dotfiles push`.

Check what would go along first:

```bash
dots status -v
```

Custom commit message:

```bash
dots push -m "update nvim keybindings"
```

### Pulling changes

When another machine has pushed updates:

```bash
dots pull
```

Fetches from the remote, shows which files changed, and lets you interactively select which ones to apply to your system. Files for other platforms are automatically skipped.

### Linking configs

On a fresh machine after cloning, or to restore all configs from the repo:

```bash
dots link
```

Copies every platform-relevant config from the repo to its system location. Skips files that already exist unless you force it:

```bash
dots link --force
```

### Watching for changes

For hands-free syncing:

```bash
dots watch
```

Runs in the foreground, watching all tracked files. When you save a change, it debounces (3 seconds by default), copies to repo, commits, and pushes. It also polls the remote periodically for incoming changes.

Custom poll interval (in minutes):

```bash
dots watch --poll-interval 10
```

Press `Ctrl+C` to stop.

## Scanning for AI Agent Configs

Bulk-add configs from AI coding tools you have installed:

```bash
dots scan
```

This scans for known config locations of:

| Agent | Paths checked |
|---|---|
| Claude Code | `~/.claude/agents/`, `~/.claude/commands/`, `~/.claude/settings.json`, `~/.agents/skills/` |
| GitHub Copilot | `~/.config/github-copilot/` |
| Codex | `~/.codex/` |
| Gemini | `~/.gemini/` |
| Continue.dev | `~/.continue/` |

Only paths that actually exist on your machine and aren't already tracked are shown. You pick which ones to add via checkboxes.

**Scan a specific agent only:**

```bash
dots scan --target claude
```

**Set platforms for the added entries:**

```bash
dots scan -P linux,macos
```

By default, scanned entries are set to all platforms.

## How It Works

### Config file

Everything lives in `dots.toml` at the root of your dotfiles repo:

```toml
[repo]
remote = "origin"

[watch]
debounce_secs = 3

[entries.shared]
"~/.claude/settings.json" = "shared/.claude/settings.json"

[entries.linux]
"~/.config/nvim" = "shared/nvim"
"~/.bashrc" = "shared/shell/.bashrc"

[entries.macos]
"~/.config/nvim" = "shared/nvim"
"~/.bashrc" = "shared/shell/.bashrc"

[entries.windows]

[rsync]
source = ""
host = ""
dest = ""
```

Each line maps a system path to a location in the repo. The section says which platforms it applies to: `shared` means every platform, and listing the same path under `linux` and `macos` shares it between those two. The older `[[entry]]` format with a `platforms` list is migrated to this layout automatically.

### Repo structure

```
~/dotfiles/
  dots.toml
  shared/          # configs used on both linux and macos
    nvim/
    ghostty/
  linux/           # linux-only configs
    hypr/
  macos/           # macos-only configs
  windows/         # windows configs
```

### Change detection

Uses SHA256 hashing to compare files. For directories, it recursively compares all files and detects added/removed/changed files.

### Git authentication

**SSH URLs** (`git@github.com:you/dotfiles.git`) — recommended. Tries in order:
1. SSH agent (keys already loaded)
2. `~/.ssh/id_ed25519`
3. `~/.ssh/id_rsa`

**HTTPS URLs** (`https://github.com/you/dotfiles.git`) — requires env vars:
```bash
GIT_USERNAME=you GIT_PASSWORD=<github-PAT> dots init https://github.com/you/dotfiles.git
```
GitHub no longer accepts account passwords over HTTPS; use a [Personal Access Token](https://github.com/settings/tokens) with `repo` scope.

### Environment variables

- `DOTS_REPO` — override the dotfiles repo location (default: `~/dotfiles`)

## Quick Reference

```
dots init [REMOTE] [--path PATH]    Set up or clone a dotfiles repo
dots add PATH [-P PLATFORMS]        Track a file or directory
dots scan [--target AGENT] [-P]     Bulk-add AI agent configs
dots status [-v] [--no-fetch]       Show sync state of all entries
dots push [-m MESSAGE]              Commit and push local changes
dots pull                           Fetch and apply remote changes
dots link [--force]                 Copy repo configs to system
dots watch [--poll-interval MIN]    Auto-sync in the background
dots rsync [PATH] [--host] [--dest] [--dry-run]
                                    Rsync a folder to another machine over SSH
dots config                         Open dots.toml in your editor
dots update [--check]               Update dots to the latest release
```
