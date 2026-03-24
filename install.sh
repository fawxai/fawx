#!/usr/bin/env bash
set -euo pipefail

# Fawx — Install Script
# Builds the engine and installs `fawx` as a standalone command.

INSTALL_DIR="${FAWX_INSTALL_DIR:-$HOME/.local/bin}"
BOLD="\033[1m"
GREEN="\033[0;32m"
YELLOW="\033[0;33m"
RED="\033[0;31m"
RESET="\033[0m"

info()  { echo -e "${BOLD}${GREEN}▸${RESET} $1"; }
warn()  { echo -e "${BOLD}${YELLOW}▸${RESET} $1"; }
error() { echo -e "${BOLD}${RED}▸${RESET} $1" >&2; }

echo -e "\n${BOLD}Fawx — Install${RESET}\n"

# Check for Rust toolchain
if ! command -v cargo &>/dev/null; then
    error "Rust toolchain not found."
    echo "  Install it: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Find repo root (script must be in repo root)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [[ ! -f "$SCRIPT_DIR/Cargo.toml" ]] || [[ ! -d "$SCRIPT_DIR/engine/crates" ]]; then
    error "Can't find Cargo.toml or engine/crates/. Run this script from the fawx repo root."
    exit 1
fi

cd "$SCRIPT_DIR"

# Build release binary
info "Building fawx (release)..."
cargo build --release -p fx-cli 2>&1

BINARY="target/release/fawx"
if [[ ! -f "$BINARY" ]]; then
    error "Build succeeded but binary not found at $BINARY"
    exit 1
fi

# Create install directory
mkdir -p "$INSTALL_DIR"

# Install
info "Installing to $INSTALL_DIR/fawx..."
cp "$BINARY" "$INSTALL_DIR/fawx"
chmod +x "$INSTALL_DIR/fawx"

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

VERSION=$("$INSTALL_DIR/fawx" --version 2>/dev/null || echo "unknown")
echo ""
info "Installed: fawx ($VERSION)"
echo ""
echo "  Run:     fawx"
echo "  Update:  git pull && ./install.sh"
echo ""
