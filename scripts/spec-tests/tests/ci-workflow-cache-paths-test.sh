#!/usr/bin/env bash
set -euo pipefail

SCRIPT="scripts/spec-tests/ci-workflow-cache-paths-check.sh"
WORKFLOW_FILE=".github/workflows/ci.yml"
TEST_FIXTURES_DIR="scripts/spec-tests/tests/fixtures/ci-cache-paths"

if [[ ! -x "$SCRIPT" ]]; then
  echo "missing executable script: $SCRIPT" >&2
  exit 1
fi

if [[ ! -f "$WORKFLOW_FILE" ]]; then
  echo "missing workflow file: $WORKFLOW_FILE" >&2
  exit 1
fi

if [[ ! -d "$TEST_FIXTURES_DIR" ]]; then
  echo "missing fixtures directory: $TEST_FIXTURES_DIR" >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1 && ! command -v grep >/dev/null 2>&1; then
  echo "test harness requires rg or grep" >&2
  exit 1
fi

if command -v rg >/dev/null 2>&1; then
  ASSERT_BIN="rg"
else
  ASSERT_BIN="grep"
fi

file_contains_fixed() {
  local file="$1"
  local pattern="$2"
  if [[ "$ASSERT_BIN" == "rg" ]]; then
    rg -q --fixed-strings -- "$pattern" "$file"
  else
    grep -F -q -- "$pattern" "$file"
  fi
}

run_expect_success() {
  local name="$1"
  shift
  local out_file
  out_file="$(mktemp)"
  if "$@" >"$out_file" 2>&1; then
    echo "PASS: $name"
  else
    echo "FAIL: $name" >&2
    cat "$out_file" >&2
    rm -f "$out_file"
    exit 1
  fi
  rm -f "$out_file"
}

run_expect_failure() {
  local name="$1"
  local expected_error="$2"
  shift
  shift
  local out_file
  out_file="$(mktemp)"
  if "$@" >"$out_file" 2>&1; then
    echo "FAIL: $name (unexpected success)" >&2
    cat "$out_file" >&2
    rm -f "$out_file"
    exit 1
  elif ! file_contains_fixed "$out_file" "$expected_error"; then
    echo "FAIL: $name (unexpected failure output)" >&2
    echo "expected to find: $expected_error" >&2
    echo "actual output:" >&2
    cat "$out_file" >&2
    rm -f "$out_file"
    exit 1
  else
    echo "PASS: $name"
  fi
  rm -f "$out_file"
}

FIXTURES_DIR="$(mktemp -d)"
trap 'rm -rf "$FIXTURES_DIR"' EXIT

cat >"$FIXTURES_DIR/cache-paths-pass-safe-user-paths.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: ~/.cargo/registry
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/git
            target
      - uses: actions/cache@v4
        with:
          path:
            - ~/.gradle/caches
            - ~/.gradle/wrapper
EOF

cat >"$FIXTURES_DIR/cache-paths-pass-inline-list.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: [~/.cargo/registry, ~/.cargo/git, target]
      - uses: actions/cache@v4
        with:
          path:
            - [~/.gradle/caches, ~/.gradle/wrapper]
EOF

cat >"$FIXTURES_DIR/cache-paths-fail-single-line.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: /var/cache/cargo
EOF

cat >"$FIXTURES_DIR/cache-paths-fail-block-scalar.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            /usr/local/cache
EOF

cat >"$FIXTURES_DIR/cache-paths-fail-list.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path:
            - ~/.cargo/git
            - /etc/ssl
EOF

cat >"$FIXTURES_DIR/cache-paths-fail-inline-list.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: [~/.cargo/registry, /var/cache/cargo]
      - uses: actions/cache@v4
        with:
          path:
            - [~/.gradle/caches, /etc/ssl]
EOF

cat >"$FIXTURES_DIR/cache-paths-fail-inline-list-quoted.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: ["~/.cargo/registry", "/etc/ssl"]
EOF

cat >"$FIXTURES_DIR/cache-paths-pass-lookalike-prefix.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@v4
        with:
          path: /varnish/cache
      - uses: actions/cache@v4
        with:
          path: [/etcetera/cache, /usrlocal/cache, /optimize/cache, /rooted/cache]
EOF

cat >"$FIXTURES_DIR/cache-paths-pass-non-cache-action-path.yml" <<'EOF'
name: CI
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/upload-artifact@v4
        with:
          name: app-logs
          path: /var/log/app.log
EOF

run_expect_success "safe user-writable cache paths pass" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-pass-safe-user-paths.yml" "$SCRIPT"

run_expect_success "inline-list safe cache paths pass" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-pass-inline-list.yml" "$SCRIPT"

run_expect_success "lookalike system-path prefixes do not match" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-pass-lookalike-prefix.yml" "$SCRIPT"

run_expect_success "non-cache action path is out of scope" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-pass-non-cache-action-path.yml" "$SCRIPT"

run_expect_failure "single-line unsafe absolute path fails" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $FIXTURES_DIR/cache-paths-fail-single-line.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-fail-single-line.yml" "$SCRIPT"

run_expect_failure "multiline block scalar unsafe path fails" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $FIXTURES_DIR/cache-paths-fail-block-scalar.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-fail-block-scalar.yml" "$SCRIPT"

run_expect_failure "list-form unsafe path fails" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $FIXTURES_DIR/cache-paths-fail-list.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-fail-list.yml" "$SCRIPT"

run_expect_failure "inline-list unsafe path fails" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $FIXTURES_DIR/cache-paths-fail-inline-list.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-fail-inline-list.yml" "$SCRIPT"

run_expect_failure "quoted inline-list unsafe path fails" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $FIXTURES_DIR/cache-paths-fail-inline-list-quoted.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$FIXTURES_DIR/cache-paths-fail-inline-list-quoted.yml" "$SCRIPT"

run_expect_failure "restore-keys list before unsafe path remains in cache-step scope" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $TEST_FIXTURES_DIR/cache-paths-fail-restore-keys-before-path.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$TEST_FIXTURES_DIR/cache-paths-fail-restore-keys-before-path.yml" "$SCRIPT"

run_expect_failure "cache restore/save actions with unsafe paths fail" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $TEST_FIXTURES_DIR/cache-paths-fail-cache-restore-save-actions.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$TEST_FIXTURES_DIR/cache-paths-fail-cache-restore-save-actions.yml" "$SCRIPT"

run_expect_failure "path before uses in cache step still fails" \
  "[H23-CI-CACHE-001] disallowed cache path detected in $TEST_FIXTURES_DIR/cache-paths-fail-path-before-uses.yml: cache paths must be user-writable" \
  env WORKFLOW_FILE="$TEST_FIXTURES_DIR/cache-paths-fail-path-before-uses.yml" "$SCRIPT"

run_expect_success "repo workflow cache paths remain safe" \
  env WORKFLOW_FILE="$WORKFLOW_FILE" "$SCRIPT"

echo "ci workflow cache-path safety tests passed"
