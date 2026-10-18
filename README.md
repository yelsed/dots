# dots

Cross-platform dotfile synchronization tool. Track config files across machines using a git-backed repository with platform-aware entries, change detection via SHA256 hashing, and both one-shot and continuous sync modes.

## Install

### Quick install (Linux / macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/yelsed/dots/master/install.sh | sh
```

### Download from GitHub Releases

Grab the latest binary for your platform from the [Releases page](https://github.com/yelsed/dots/releases):

| Platform | Asset |
|---|---|
| Linux (x86_64) | `dots-x86_64-unknown-linux-gnu.tar.gz` |
| macOS (Apple Silicon) | `dots-aarch64-apple-darwin.tar.gz` |
| Windows | `dots-x86_64-pc-windows-msvc.zip` |

Extract and move the binary somewhere on your PATH (e.g. `~/.local/bin`).

### From source

```sh
cargo install --git https://github.com/yelsed/dots.git
```

## Usage

### Initialize

Clone an existing dotfiles repo or create a new one:

```sh
dots init                                        # Create ~/dotfiles from scratch
dots init git@github.com:you/dotfiles.git        # Clone existing repo (SSH — recommended)
dots init https://github.com/you/dotfiles.git   # Clone existing repo (HTTPS — requires GIT_USERNAME/GIT_PASSWORD)
```

> **SSH vs HTTPS:** SSH is recommended. HTTPS clones require `GIT_USERNAME` and `GIT_PASSWORD` env vars (use a GitHub Personal Access Token as the password). SSH uses your local key (`~/.ssh/id_ed25519` or `~/.ssh/id_rsa`) or SSH agent automatically.

### Track files

Start tracking a config file or directory:

```sh
dots add ~/.bashrc
dots add ~/.config/nvim -P linux,macos
```

### Sync

Push local changes to the repo, or pull remote changes to your system:

```sh
dots push                  # Copy tracked changes to repo, commit, push
dots push -m "message"     # Same, with your own commit message
dots pull                  # Pull remote changes, interactively apply
```

A typical round trip between two machines:

```sh
# machine A, after editing configs
dots status -v             # see exactly which files changed
dots push

# machine B
dots pull                  # pick which changed entries to apply
```

How push and pull behave:

- `dots push` only adds and overwrites. A file that exists in the repo but no longer on this machine stays in the repo. Remove it from the repo with git if it should go.
- `dots push` does not pull first. When the remote has commits you do not have, the commit is made locally and the push fails. Run `git -C ~/dotfiles pull --ff-only` (or `dots pull`) and push again with `git -C ~/dotfiles push`.
- `dots pull` only fast-forwards. If the repo has local commits the remote does not have, resolve it with git by hand.
- `dots pull` is interactive, so run it in a real terminal.

### Watch

Run a background watcher that auto-syncs on file changes and periodically polls the remote:

```sh
dots watch
dots watch --poll-interval 15   # Poll every 15 minutes
```

### Status

See what's changed across all tracked entries:

```sh
dots status                # One line per tracked entry, plus remote state
dots status -v             # Also list the individual files that differ inside directories
dots status --no-fetch     # Skip contacting the remote
```

In the verbose list, `~` is a changed file, `+` a file only on this machine and `-` a file only in the repo.

### Scan for AI agent configs

```sh
dots scan                  # Find Claude, Copilot, Codex, Gemini and Continue configs and pick which to track
dots scan --target claude
```

Scanned configs can contain login tokens (for example `~/.config/github-copilot/auth.db`). Keep the dotfiles repo private, or leave those entries out.

### Rsync a folder

Send a folder that is too big or too private for git straight to another machine over SSH:

```sh
dots rsync                                     # Uses the [rsync] block in dots.toml
dots rsync ~/folder --host user@host --dest /path/on/remote
dots rsync --dry-run
```

### Edit the config and update dots

```sh
dots config                # Open dots.toml in your editor
dots update                # Install the latest release
dots update --check        # Only check for a newer release
```

### Link

Copy all platform-relevant configs from the repo to your system:

```sh
dots link
dots link --force          # Overwrite existing files
```

## Configuration

Configuration lives at `~/dotfiles/dots.toml` (override the repo location with the `DOTS_REPO` env var). Entries are grouped by platform, each line maps a system path to a path in the repo:

```toml
[repo]
remote = "origin"          # git remote name, not a URL

[watch]
debounce_secs = 3

[entries.shared]           # every platform
"~/.config/nvim" = "shared/nvim"

[entries.macos]            # macOS only
"~/.zshrc" = "shared/.zshrc"

[entries.linux]            # Linux only
"~/.config/hypr/bindings.conf" = "linux/hypr/bindings.conf"

[entries.windows]

[rsync]
source = "~/folder-to-send"
host = "user@192.168.1.100"
dest = "/home/user/backup-folder"
```

The same source path may appear under both `linux` and `macos` to share it between those two without Windows. A `dots.toml` in the older `[[entry]]` format is converted to this format automatically the first time it is loaded.

### Environment variables

| Variable | Purpose |
|---|---|
| `DOTS_REPO` | Override default repo path (`~/dotfiles`) |
| `GIT_USERNAME` | HTTPS auth username (not needed for SSH) |
| `GIT_PASSWORD` | HTTPS auth password/token (use a GitHub PAT, not your account password) |

## Development

```sh
cargo build              # Debug build
cargo build --release    # Release build
cargo run -- status      # Run directly
```

## License

[MIT](LICENSE)
