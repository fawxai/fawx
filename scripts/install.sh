#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

main() {
    echo "Building Fawx..."
    cd "$REPO_ROOT"
    cargo build --release -p fx-cli

    echo "Installing to $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
    cp "target/release/fawx" "$INSTALL_DIR/fawx"
    chmod +x "$INSTALL_DIR/fawx"

    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        echo ""
        echo "⚠ $INSTALL_DIR is not in your PATH."
        echo "  Add it to your shell profile:"
        echo ""
        echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
        echo ""
        echo "  Then restart your shell or run: source ~/.bashrc"
    fi

    echo ""
    echo "✓ Fawx installed. Run: fawx setup"
}

main "$@"
