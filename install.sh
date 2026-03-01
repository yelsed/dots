#!/bin/sh
set -e

REPO="yelsed/dots"
INSTALL_DIR="$HOME/.local/bin"

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux)  OS_TARGET="x86_64-unknown-linux-gnu" ;;
    Darwin)  OS_TARGET="aarch64-apple-darwin" ;;
    *)  echo "Unsupported OS: $OS (try downloading manually from GitHub Releases)"; exit 1 ;;
esac

# Get latest release tag
TAG="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)"
if [ -z "$TAG" ]; then
    echo "Failed to fetch latest release tag"
    exit 1
fi

echo "Installing dots $TAG for $OS_TARGET..."

# Download and extract
URL="https://github.com/$REPO/releases/download/$TAG/dots-$OS_TARGET.tar.gz"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "$TMPDIR/dots.tar.gz"
tar -xzf "$TMPDIR/dots.tar.gz" -C "$TMPDIR"

# Install
mkdir -p "$INSTALL_DIR"
mv "$TMPDIR/dots" "$INSTALL_DIR/dots"
chmod +x "$INSTALL_DIR/dots"

echo "Installed dots to $INSTALL_DIR/dots"

# Check if install dir is in PATH
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "Add $INSTALL_DIR to your PATH:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        echo ""
        echo "To make this permanent, add the line above to your ~/.bashrc or ~/.zshrc"
        ;;
esac
