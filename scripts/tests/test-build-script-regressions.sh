#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUILD_SCRIPT="$ROOT_DIR/scripts/build.sh"
SKILLS_BUILD_SCRIPT="$ROOT_DIR/skills/build.sh"
LIB_SCRIPT="$ROOT_DIR/scripts/lib.sh"

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

assert_logged_invocation() {
  local log_file="$1"
  local expected="$2"
  grep -Fq "$expected" "$log_file" || {
    cat "$log_file" >&2
    fail "missing log entry: $expected"
  }
}

TMP_DIR="$(mktemp -d)"
FAKE_BIN="$TMP_DIR/bin"
FAKE_HOME="$TMP_DIR/home"
CARGO_LOG="$TMP_DIR/cargo.log"
export CARGO_LOG
mkdir -p "$FAKE_BIN" "$FAKE_HOME/.cargo/bin"
trap 'rm -rf "$TMP_DIR"' EXIT

require_contains "$BUILD_SCRIPT" 'source "$SCRIPT_DIR/lib.sh"'
require_contains "$SKILLS_BUILD_SCRIPT" 'source "$SCRIPT_DIR/../scripts/lib.sh"'
require_contains "$LIB_SCRIPT" 'detect_cpu_count()'
require_contains "$LIB_SCRIPT" 'resolve_tool()'

make_fake_command "$FAKE_BIN/dirname" 'exec /usr/bin/dirname "$@"'
make_fake_command "$FAKE_BIN/date" 'exec /usr/bin/date "$@"'
make_fake_command "$FAKE_BIN/awk" 'exec /usr/bin/awk "$@"'
make_fake_command "$FAKE_BIN/cat" 'exec /usr/bin/cat "$@"'
make_fake_command "$FAKE_BIN/grep" 'exec /usr/bin/grep "$@"'
make_fake_command "$FAKE_BIN/cargo" '
{
  printf "argc=%s\n" "$#"
  for arg in "$@"; do
    printf "arg=%s\n" "$arg"
  done
  printf -- "---\n"
} >>"$CARGO_LOG"
'
make_fake_command "$FAKE_HOME/.cargo/bin/rustup" 'echo rustup-fallback >/dev/null'

PATH="$FAKE_BIN" HOME="$FAKE_HOME" /bin/bash "$BUILD_SCRIPT" --check >/dev/null
PATH="$FAKE_BIN" HOME="$FAKE_HOME" /bin/bash "$SKILLS_BUILD_SCRIPT" --help >/dev/null

assert_logged_invocation "$CARGO_LOG" 'arg=fmt'
assert_logged_invocation "$CARGO_LOG" 'arg=clippy'
assert_logged_invocation "$CARGO_LOG" 'arg=test'
assert_logged_invocation "$CARGO_LOG" 'arg=--workspace'
assert_logged_invocation "$CARGO_LOG" 'arg=--exclude'
assert_logged_invocation "$CARGO_LOG" 'arg=llama-cpp-sys'

if grep -Fq 'arg=--workspace --exclude llama-cpp-sys' "$CARGO_LOG"; then
  cat "$CARGO_LOG" >&2
  fail 'workspace check args were collapsed into one word-split string'
fi

echo "build script regression checks passed"
