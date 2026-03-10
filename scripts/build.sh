#!/usr/bin/env bash
set -euo pipefail

# Fawx Build Script
# Builds engine, TUI, and all WASM skills
#
# Usage:
#   ./scripts/build.sh              # Build debug
#   ./scripts/build.sh --release    # Build release
#   ./scripts/build.sh --skills     # Build only WASM skills
#   ./scripts/build.sh --engine     # Build only engine + TUI
#   ./scripts/build.sh --check      # Format + clippy + test (CI check)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=./lib.sh
source "$SCRIPT_DIR/lib.sh"
MODE="debug"
PROFILE="debug"
INSTALL=false
CARGO_ARGS=()
BUILD_SCOPE="all"
TOTAL_STEPS=3
START_TIME=0
CARGO_BUILD_JOBS_VALUE=""
CARGO_BIN=""
RELEASE_REQUESTED=0
CHECK_REQUESTED=0
WORKSPACE_CHECK_ARGS=()

usage() {
  cat <<'EOF'
Usage:
  ./scripts/build.sh
  ./scripts/build.sh --release
  ./scripts/build.sh --skills
  ./scripts/build.sh --engine
  ./scripts/build.sh --check
  ./scripts/build.sh --install
EOF
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --release)
        RELEASE_REQUESTED=1
        MODE="release"
        PROFILE="release"
        CARGO_ARGS=(--release)
        ;;
      --skills)
        BUILD_SCOPE="skills"
        TOTAL_STEPS=1
        ;;
      --engine)
        BUILD_SCOPE="engine"
        TOTAL_STEPS=2
        ;;
      --check)
        CHECK_REQUESTED=1
        BUILD_SCOPE="check"
        MODE="check"
        PROFILE="debug"
        CARGO_ARGS=()
        TOTAL_STEPS=1
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

validate_args() {
  if (( RELEASE_REQUESTED == 1 && CHECK_REQUESTED == 1 )); then
    echo "--release cannot be combined with --check" >&2
    exit 1
  fi
}

has_cxx_compiler() {
  command -v c++ >/dev/null 2>&1 || \
    command -v clang++ >/dev/null 2>&1 || \
    command -v g++ >/dev/null 2>&1
}

set_workspace_check_args() {
  WORKSPACE_CHECK_ARGS=(--workspace)
  if has_cxx_compiler; then
    return
  fi
  WORKSPACE_CHECK_ARGS+=(--exclude llama-cpp-sys)
}

step_header() {
  local step="$1"
  local message="$2"
  printf '\n[%s/%s] %s\n' "$step" "$TOTAL_STEPS" "$message"
}

build_engine() {
  step_header "$1" "Building engine (fawx)..."
  (
    cd "$REPO_ROOT"
    "$CARGO_BIN" build -p fx-cli -j "$CARGO_BUILD_JOBS_VALUE" ${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"}
  )
  echo "✓ fawx built (target/$PROFILE/fawx)"
}

build_tui() {
  step_header "$1" "Building TUI (fawx-tui)..."
  (
    cd "$REPO_ROOT"
    "$CARGO_BIN" build -p fawx-tui --features embedded -j "$CARGO_BUILD_JOBS_VALUE" ${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"}
  )
  echo "✓ fawx-tui built (target/$PROFILE/fawx-tui)"
}

count_skills() {
  local count=0
  local manifest
  for manifest in "$REPO_ROOT"/skills/*/Cargo.toml; do
    [[ -f "$manifest" ]] || continue
    count=$((count + 1))
  done
  echo "$count"
}

build_skills() {
  local count
  local skills_args=(${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"})
  count="$(count_skills)"

  if [[ "$INSTALL" == true ]]; then
    skills_args+=(--install)
  fi

  step_header "$1" "Building WASM skills..."
  (
    cd "$REPO_ROOT/skills"
    CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS_VALUE" ./build.sh ${skills_args[@]+"${skills_args[@]}"}
  )
  echo "✓ $count skills built"
}

run_checks() {
  step_header 1 "Running format, clippy, and tests..."
  set_workspace_check_args
  (
    cd "$REPO_ROOT"
    "$CARGO_BIN" fmt --all --check
    "$CARGO_BIN" clippy ${WORKSPACE_CHECK_ARGS[@]+"${WORKSPACE_CHECK_ARGS[@]}"} -- -D warnings
    "$CARGO_BIN" test ${WORKSPACE_CHECK_ARGS[@]+"${WORKSPACE_CHECK_ARGS[@]}"}
  )
  if ! has_cxx_compiler; then
    echo "✓ checks passed (llama-cpp-sys skipped: no C++ compiler found)"
    return
  fi
  echo "✓ checks passed"
}

version_from_cargo_toml() {
  awk '
    /^\[workspace.package\]$/ { in_section=1; next }
    /^\[/ { in_section=0 }
    in_section && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
  ' "$REPO_ROOT/Cargo.toml"
}

format_duration() {
  local total="$1"
  local hours minutes seconds
  hours=$((total / 3600))
  minutes=$(((total % 3600) / 60))
  seconds=$((total % 60))
  if (( hours > 0 )); then
    printf '%dh %dm %ds' "$hours" "$minutes" "$seconds"
    return
  fi
  if (( minutes > 0 )); then
    printf '%dm %ds' "$minutes" "$seconds"
    return
  fi
  printf '%ds' "$seconds"
}

print_summary() {
  local elapsed version
  elapsed=$(($(date +%s) - START_TIME))
  version="$(version_from_cargo_toml)"
  echo
  echo "Build complete in $(format_duration "$elapsed")"
  echo "Version: $version"
  echo "Jobs: $CARGO_BUILD_JOBS_VALUE"
}

print_header() {
  echo "Fawx Build Script"
  echo "─────────────────"
  echo "Mode: $MODE"
  echo
}

main() {
  CARGO_BUILD_JOBS_VALUE="${CARGO_BUILD_JOBS:-$(detect_cpu_count)}"
  CARGO_BIN="$(resolve_tool cargo)"
  parse_args "$@"
  validate_args
  START_TIME="$(date +%s)"
  print_header

  case "$BUILD_SCOPE" in
    all)
      build_engine 1
      build_tui 2
      build_skills 3
      ;;
    engine)
      build_engine 1
      build_tui 2
      ;;
    skills)
      build_skills 1
      ;;
    check)
      run_checks
      ;;
  esac

  print_summary
}

main "$@"
