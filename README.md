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
dots init                          # Create ~/dotfiles from scratch
dots init https://github.com/you/dotfiles.git  # Clone existing repo
```

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
dots pull                  # Pull remote changes, interactively apply
```

### Watch

Run a background watcher that auto-syncs on file changes and periodically polls the remote:

```sh
dots watch
dots watch --poll-interval 15   # Poll every 15 minutes
```

### Status

See what's changed across all tracked entries:

```sh
dots status
```

### Link

Copy all platform-relevant configs from the repo to your system:

```sh
dots link
dots link --force          # Overwrite existing files
```

## Configuration

Configuration lives at `~/dotfiles/dots.toml` (override with `DOTS_REPO` env var).

```toml
[repo]
path = "~/dotfiles"
remote = "git@github.com:you/dotfiles.git"

[[entries]]
source = "~/.bashrc"
repo_path = "bashrc"
platforms = ["linux", "macos"]

[[entries]]
source = "~/.config/nvim"
repo_path = "nvim"
platforms = ["linux", "macos"]
```

### Environment variables

| Variable | Purpose |
|---|---|
| `DOTS_REPO` | Override default repo path (`~/dotfiles`) |
| `GIT_USERNAME` | Git credential fallback (username) |
| `GIT_PASSWORD` | Git credential fallback (password/token) |

## Development

```sh
cargo build              # Debug build
cargo build --release    # Release build
cargo run -- status      # Run directly
```

## License

[MIT](LICENSE)
