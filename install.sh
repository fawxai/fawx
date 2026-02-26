#!/usr/bin/env bash
set -euo pipefail

# Citros — Install Script
# Builds the engine and installs `citros` as a standalone command.

INSTALL_DIR="${CITROS_INSTALL_DIR:-$HOME/.local/bin}"
BOLD="\033[1m"
GREEN="\033[0;32m"
YELLOW="\033[0;33m"
RED="\033[0;31m"
RESET="\033[0m"

info()  { echo -e "${BOLD}${GREEN}▸${RESET} $1"; }
warn()  { echo -e "${BOLD}${YELLOW}▸${RESET} $1"; }
error() { echo -e "${BOLD}${RED}▸${RESET} $1" >&2; }

echo -e "\n${BOLD}Citros — Install${RESET}\n"

# Check for Rust toolchain
if ! command -v cargo &>/dev/null; then
    error "Rust toolchain not found."
    echo "  Install it: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Find repo root (script must be in repo root)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [[ ! -f "$SCRIPT_DIR/engine/Cargo.toml" ]]; then
    error "Can't find engine/Cargo.toml. Run this script from the citros repo root."
    exit 1
fi

cd "$SCRIPT_DIR"

# Build release binary
info "Building citros (release)..."
cargo build --release -p ct-cli 2>&1

BINARY="target/release/citros"
if [[ ! -f "$BINARY" ]]; then
    error "Build succeeded but binary not found at $BINARY"
    exit 1
fi

# Create install directory
mkdir -p "$INSTALL_DIR"

# Install
info "Installing to $INSTALL_DIR/citros..."
cp "$BINARY" "$INSTALL_DIR/citros"
chmod +x "$INSTALL_DIR/citros"

# Check if install dir is in PATH
if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
    warn "$INSTALL_DIR is not in your PATH."
    echo ""
    echo "  Add it to your shell config:"
    echo ""
    echo "    echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.zshrc"
    echo "    source ~/.zshrc"
    echo ""
fi

VERSION=$("$INSTALL_DIR/citros" --version 2>/dev/null || echo "unknown")
echo ""
info "Installed: citros ($VERSION)"
echo ""
echo "  Run:     citros"
echo "  Update:  git pull && ./install.sh"
echo ""
