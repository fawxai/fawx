#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUILD_SCRIPT="$ROOT_DIR/scripts/build.sh"
SKILLS_BUILD_SCRIPT="$ROOT_DIR/skills/build.sh"
LIB_SCRIPT="$ROOT_DIR/scripts/lib.sh"
SKILL_WASM_TARGET="wasm32-wasip1"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

require_contains() {
  local file="$1"
  local expected="$2"
  grep -Fq "$expected" "$file" || fail "$file missing: $expected"
}

make_fake_command() {
  local path="$1"
  local body="$2"
  cat >"$path" <<EOF
#!/bin/bash
set -euo pipefail
$body
EOF
  chmod +x "$path"
}

assert_exists() {
  local path="$1"
  [[ -f "$path" ]] || fail "missing file: $path"
}

assert_logged_invocation() {
  local log_file="$1"
  local expected="$2"
  grep -Fq "$expected" "$log_file" || {
    cat "$log_file" >&2
    fail "missing log entry: $expected"
  }
}

assert_skill_install_artifacts() {
  local install_dir="$1"
  local directory artifact
  while IFS=: read -r directory artifact; do
    assert_exists "$install_dir/$directory/$artifact"
    assert_exists "$install_dir/$directory/manifest.toml"
  done <<'EOF'
weather-skill:weather.wasm
calculator-skill:calculator.wasm
vision-skill:vision.wasm
tts-skill:tts.wasm
browser-skill:browser.wasm
stt-skill:stt.wasm
canvas-skill:canvas.wasm
github-skill:github.wasm
EOF
}

TMP_DIR="$(mktemp -d)"
FAKE_BIN="$TMP_DIR/bin"
FAKE_HOME="$TMP_DIR/home"
CARGO_LOG="$TMP_DIR/cargo.log"
REAL_AWK="$(command -v awk)"
REAL_BASH="$(command -v bash)"
REAL_CAT="$(command -v cat)"
REAL_CP="$(command -v cp)"
REAL_DATE="$(command -v date)"
REAL_DIRNAME="$(command -v dirname)"
REAL_GREP="$(command -v grep)"
REAL_MKDIR="$(command -v mkdir)"
export CARGO_LOG
mkdir -p "$FAKE_BIN" "$FAKE_HOME/.cargo/bin"
trap 'rm -rf "$TMP_DIR"' EXIT

require_contains "$BUILD_SCRIPT" 'source "$SCRIPT_DIR/lib.sh"'
require_contains "$BUILD_SCRIPT" 'local skills_args=(${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"})'
require_contains "$BUILD_SCRIPT" './build.sh ${skills_args[@]+"${skills_args[@]}"}'
require_contains "$BUILD_SCRIPT" 'clippy ${WORKSPACE_CHECK_ARGS[@]+"${WORKSPACE_CHECK_ARGS[@]}"} -- -D warnings'
require_contains "$BUILD_SCRIPT" 'test ${WORKSPACE_CHECK_ARGS[@]+"${WORKSPACE_CHECK_ARGS[@]}"}'
require_contains "$SKILLS_BUILD_SCRIPT" 'source "$SCRIPT_DIR/../scripts/lib.sh"'
require_contains "$SKILLS_BUILD_SCRIPT" "\"\$CARGO_BIN\" build --target $SKILL_WASM_TARGET -j \"\$CARGO_BUILD_JOBS_VALUE\" \${CARGO_ARGS[@]+\"\${CARGO_ARGS[@]}\"}"
require_contains "$LIB_SCRIPT" 'detect_cpu_count()'
require_contains "$LIB_SCRIPT" 'resolve_tool()'

make_fake_command "$FAKE_BIN/bash" 'exec "'"$REAL_BASH"'" "$@"'
make_fake_command "$FAKE_BIN/dirname" 'exec "'"$REAL_DIRNAME"'" "$@"'
make_fake_command "$FAKE_BIN/date" 'exec "'"$REAL_DATE"'" "$@"'
make_fake_command "$FAKE_BIN/awk" 'exec "'"$REAL_AWK"'" "$@"'
make_fake_command "$FAKE_BIN/cat" 'exec "'"$REAL_CAT"'" "$@"'
make_fake_command "$FAKE_BIN/grep" 'exec "'"$REAL_GREP"'" "$@"'
make_fake_command "$FAKE_BIN/mkdir" 'exec "'"$REAL_MKDIR"'" "$@"'
make_fake_command "$FAKE_BIN/cp" 'exec "'"$REAL_CP"'" "$@"'
make_fake_command "$FAKE_BIN/cargo" '
{
  printf "argc=%s\n" "$#"
  for arg in "$@"; do
    printf "arg=%s\n" "$arg"
  done
  printf -- "---\n"
} >>"$CARGO_LOG"

if [[ "${1:-}" != "build" ]]; then
  exit 0
fi

profile=debug
for arg in "$@"; do
  if [[ "$arg" == "--release" ]]; then
    profile=release
    break
  fi
done

crate="${PWD##*/}"
crate="${crate//-/_}"
target_dir="$PWD/target/'"$SKILL_WASM_TARGET"'/$profile"
mkdir -p "$target_dir"
printf "fake wasm for %s\n" "$crate" >"$target_dir/$crate.wasm"
'
make_fake_command "$FAKE_HOME/.cargo/bin/rustup" '
if [[ "${1:-}" == "target" && "${2:-}" == "list" && "${3:-}" == "--installed" ]]; then
  printf "'"$SKILL_WASM_TARGET"'\n"
  exit 0
fi

if [[ "${1:-}" == "target" && "${2:-}" == "add" && "${3:-}" == "'"$SKILL_WASM_TARGET"'" ]]; then
  exit 0
fi

exit 0
'

SKILLS_OUTPUT="$TMP_DIR/skills.out"
SKILLS_INSTALL_OUTPUT="$TMP_DIR/skills-install.out"

PATH="$FAKE_BIN" HOME="$FAKE_HOME" /bin/bash "$BUILD_SCRIPT" --check >/dev/null
PATH="$FAKE_BIN" HOME="$FAKE_HOME" /bin/bash "$SKILLS_BUILD_SCRIPT" --help >/dev/null
PATH="$FAKE_BIN" HOME="$FAKE_HOME" /bin/bash "$BUILD_SCRIPT" --skills >"$SKILLS_OUTPUT"
PATH="$FAKE_BIN" HOME="$FAKE_HOME" /bin/bash "$BUILD_SCRIPT" --skills --install >"$SKILLS_INSTALL_OUTPUT"

assert_logged_invocation "$CARGO_LOG" 'arg=fmt'
assert_logged_invocation "$CARGO_LOG" 'arg=clippy'
assert_logged_invocation "$CARGO_LOG" 'arg=test'
assert_logged_invocation "$CARGO_LOG" 'arg=--workspace'
assert_logged_invocation "$CARGO_LOG" 'arg=--exclude'
assert_logged_invocation "$CARGO_LOG" 'arg=llama-cpp-sys'
require_contains "$SKILLS_OUTPUT" '✓ 8 skills built'
require_contains "$SKILLS_INSTALL_OUTPUT" 'Installed to ~/.fawx/skills/'

if grep -Fq 'arg=--workspace --exclude llama-cpp-sys' "$CARGO_LOG"; then
  cat "$CARGO_LOG" >&2
  fail 'workspace check args were collapsed into one word-split string'
fi

assert_skill_install_artifacts "$FAKE_HOME/.fawx/skills"

echo "build script regression checks passed"
