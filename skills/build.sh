#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../scripts/lib.sh
source "$SCRIPT_DIR/../scripts/lib.sh"
MODE="debug"
PROFILE="debug"
INSTALL=false
INSTALL_DIR="$HOME/.fawx/skills"
CARGO_ARGS=()
CARGO_BUILD_JOBS_VALUE=""
CARGO_BIN=""
RUSTUP_BIN=""
INSTALLED_SKILLS=()
SKILL_SPECS=(
  "weather-skill:weather_skill:weather.wasm"
  "calculator-skill:calculator_skill:calculator.wasm"
  "vision-skill:vision_skill:vision.wasm"
  "tts-skill:tts_skill:tts.wasm"
  "browser-skill:browser_skill:browser.wasm"
  "stt-skill:stt_skill:stt.wasm"
  "canvas-skill:canvas_skill:canvas.wasm"
  "github-skill:github_skill:github.wasm"
)

usage() {
  cat <<'EOF'
Usage:
  ./skills/build.sh           Build WASM skills (debug)
  ./skills/build.sh --release Build WASM skills (release)
  ./skills/build.sh --install Build and install WASM skills to ~/.fawx/skills/
EOF
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --release)
        MODE="release"
        PROFILE="release"
        CARGO_ARGS=(--release)
        ;;
      --install)
        INSTALL=true
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        echo "Unknown option: $1" >&2
        usage >&2
        exit 1
        ;;
    esac
    shift
  done
}

ensure_wasm_target() {
  if "$RUSTUP_BIN" target list --installed | grep -qx 'wasm32-unknown-unknown'; then
    return
  fi
  echo "Installing wasm32-unknown-unknown target..."
  "$RUSTUP_BIN" target add wasm32-unknown-unknown
}

install_skill() {
  local directory="$1"
  local artifact="$2"
  local output="$SCRIPT_DIR/$directory/$artifact"
  local manifest="$SCRIPT_DIR/$directory/manifest.toml"
  local skill_dir="$INSTALL_DIR/$directory"

  mkdir -p "$skill_dir"
  cp "$output" "$skill_dir/$artifact"
  cp "$manifest" "$skill_dir/manifest.toml"
  INSTALLED_SKILLS+=("$directory")
}

build_skill() {
  local spec="$1"
  local directory crate artifact source output
  IFS=: read -r directory crate artifact <<<"$spec"
  source="$SCRIPT_DIR/$directory/target/wasm32-unknown-unknown/$PROFILE/$crate.wasm"
  output="$SCRIPT_DIR/$directory/$artifact"

  echo "Building $directory..."
  (
    cd "$SCRIPT_DIR/$directory"
    "$CARGO_BIN" build --target wasm32-unknown-unknown -j "$CARGO_BUILD_JOBS_VALUE" ${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"}
  )
  cp "$source" "$output"
  echo "✓ $directory built -> $directory/$artifact"

  if [[ "$INSTALL" == true ]]; then
    install_skill "$directory" "$artifact"
  fi
}

print_summary() {
  echo
  echo "✓ ${#SKILL_SPECS[@]} skills built"

  if [[ "$INSTALL" != true ]]; then
    return
  fi

  echo
  echo "Installed to ~/.fawx/skills/"
  echo "✓ ${#INSTALLED_SKILLS[@]} skills installed"
}

main() {
  CARGO_BUILD_JOBS_VALUE="${CARGO_BUILD_JOBS:-$(detect_cpu_count)}"
  CARGO_BIN="$(resolve_tool cargo)"
  RUSTUP_BIN="$(resolve_tool rustup)"
  parse_args "$@"
  ensure_wasm_target

  echo "Building Fawx skills ($MODE)..."
  for spec in "${SKILL_SPECS[@]}"; do
    build_skill "$spec"
  done

  print_summary
}

main "$@"
