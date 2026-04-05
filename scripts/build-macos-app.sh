#!/usr/bin/env bash
set -euo pipefail

# Builds the macOS app bundle at build/Fawx.app without packaging a DMG.
#
# Usage:
#   ./scripts/build-macos-app.sh
#   ./scripts/build-macos-app.sh --release
#   ./scripts/build-macos-app.sh --release --identity "Developer ID Application: ..."

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

exec "$SCRIPT_DIR/build-dmg.sh" --app-only "$@"
