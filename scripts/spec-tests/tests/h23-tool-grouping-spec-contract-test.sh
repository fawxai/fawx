#!/usr/bin/env bash
set -euo pipefail

SCRIPT="scripts/spec-tests/h23-tool-grouping-spec-contract.sh"
FIXTURE_BUILDER="scripts/spec-tests/fixtures/h23-contract-fixture-builder.sh"

if [[ ! -x "$SCRIPT" ]]; then
  echo "missing executable script: $SCRIPT" >&2
  exit 1
fi

if [[ ! -f "$FIXTURE_BUILDER" ]]; then
  echo "missing fixture builder: $FIXTURE_BUILDER" >&2
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
TMP_RG_BIN_DIR="$(mktemp -d)"
TMP_GREP_BIN_DIR="$(mktemp -d)"
TMP_EMPTY_BIN_DIR="$(mktemp -d)"
trap 'rm -rf "$FIXTURES_DIR" "$TMP_RG_BIN_DIR" "$TMP_GREP_BIN_DIR" "$TMP_EMPTY_BIN_DIR"' EXIT

# shellcheck source=/dev/null
source "$FIXTURE_BUILDER"
build_h23_contract_fixtures "$FIXTURES_DIR"

run_expect_success "fixture builder emits contract fixtures" \
  test -f "$FIXTURES_DIR/h23-contract-pass.md"

run_expect_success "valid fixture passes" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" "$SCRIPT"

run_expect_failure "invalid fixture fails" \
  "expected pattern missing in section '7.3 Acceptance criteria' of file '$FIXTURES_DIR/h23-contract-fail-missing-acceptance.md': Policy-violation counter" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-fail-missing-acceptance.md" "$SCRIPT"

run_expect_success "subsections in target heading still match" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass-with-subsection.md" "$SCRIPT"

run_expect_success "heading whitespace variations still match" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass-with-heading-whitespace.md" "$SCRIPT"

run_expect_failure "required acceptance text outside target heading fails" \
  "expected pattern missing in section '7.3 Acceptance criteria' of file '$FIXTURES_DIR/h23-contract-fail-offsection-acceptance.md': Section 9 success criteria gates #1-#5" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-fail-offsection-acceptance.md" "$SCRIPT"

run_expect_failure "missing unknown reason-code compatibility text fails" \
  "expected pattern missing in section '7.4 ResolvedToolPlan examples (normative)' of file '$FIXTURES_DIR/h23-contract-fail-missing-unknown-reason-codes.md': Unknown future \`reasonCodes\` must be ignored by clients (forward compatibility)." \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-fail-missing-unknown-reason-codes.md" "$SCRIPT"

run_expect_failure "inconsistent JSON example semantics fail" \
  "inconsistent JSON example semantics in 7.4 ResolvedToolPlan examples (normative) example #1: NAVIGATION active while user_disabled_navigation present" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-fail-inconsistent-json-example.md" "$SCRIPT"

run_expect_success "valid multi-example co-occurrence passes" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass-valid-multi-example-cooccurrence.md" "$SCRIPT"

run_expect_success "constitution fixture with required sections passes" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-pass.md" "$SCRIPT"

run_expect_failure "constitution missing 8.6 fails" \
  "expected heading missing in file '$FIXTURES_DIR/constitution-fail-missing-8-6.md': ## 8.6" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-missing-8-6.md" "$SCRIPT"

run_expect_failure "constitution section 9 heading must include subsection format" \
  "expected heading missing in file '$FIXTURES_DIR/constitution-fail-invalid-section-9-heading.md': ## 9." \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-invalid-section-9-heading.md" "$SCRIPT"

run_expect_failure "constitution heading level must be level-2" \
  "expected heading missing in file '$FIXTURES_DIR/constitution-fail-wrong-heading-level.md': ## 5.2" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-wrong-heading-level.md" "$SCRIPT"

run_expect_failure "constitution inline text must not satisfy heading requirement" \
  "expected heading missing in file '$FIXTURES_DIR/constitution-fail-inline-false-positive.md': ## 8.6" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-inline-false-positive.md" "$SCRIPT"

run_expect_failure "constitution 5.2 semantics must be enforced" \
  "expected regex missing in section '5.2 Policy Precedence' of file '$FIXTURES_DIR/constitution-fail-5-2-semantics.md': Security.*non-bypassable" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-5-2-semantics.md" "$SCRIPT"

run_expect_failure "constitution 5.3 semantics must be enforced" \
  "expected regex missing in section '5.3 Determinism' of file '$FIXTURES_DIR/constitution-fail-5-3-semantics.md': identical explicit inputs" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-5-3-semantics.md" "$SCRIPT"

run_expect_failure "constitution 7.3 semantics must be enforced" \
  "expected regex missing in section '7.3 Acceptance Criteria' of file '$FIXTURES_DIR/constitution-fail-7-3-semantics.md': measurable acceptance gates" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-7-3-semantics.md" "$SCRIPT"

run_expect_failure "constitution 8.1 semantics must be enforced" \
  "expected regex missing in section '8.1 Testing Baseline' of file '$FIXTURES_DIR/constitution-fail-8-1-semantics.md': Behavioral changes require TDD coverage" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-8-1-semantics.md" "$SCRIPT"

run_expect_failure "constitution 8.6 semantics must be enforced" \
  "expected regex missing in section '8.6 Privacy Baseline' of file '$FIXTURES_DIR/constitution-fail-8-6-semantics.md': No raw user message content" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-8-6-semantics.md" "$SCRIPT"

run_expect_failure "constitution 9 semantics must be enforced" \
  "expected regex missing in section '9. Rollout And Gates' of file '$FIXTURES_DIR/constitution-fail-9-semantics.md': promotion gates" \
  env SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" CONSTITUTION_FILE="$FIXTURES_DIR/constitution-fail-9-semantics.md" "$SCRIPT"

run_expect_failure "invalid SEARCH_BIN fails fast" \
  "invalid SEARCH_BIN: bad-bin (expected 'rg' or 'grep')" \
  env SEARCH_BIN=bad-bin SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" "$SCRIPT"

run_expect_failure "missing SPEC_FILE fails fast" \
  "missing spec file: $FIXTURES_DIR/does-not-exist.md" \
  env SPEC_FILE="$FIXTURES_DIR/does-not-exist.md" "$SCRIPT"

run_expect_failure "missing search binary fails fast" \
  "missing required search tool: need 'rg' or 'grep'" \
  env PATH="$TMP_EMPTY_BIN_DIR" /bin/bash "$SCRIPT"

if command -v rg >/dev/null 2>&1; then
  # Simulate an rg-only PATH so any accidental hard dependency on grep fails.
  ln -s "$(command -v rg)" "$TMP_RG_BIN_DIR/rg"
  ln -s "$(command -v awk)" "$TMP_RG_BIN_DIR/awk"
  ln -s "$(command -v tr)" "$TMP_RG_BIN_DIR/tr"

  run_expect_success "rg-only environment is supported" \
    env PATH="$TMP_RG_BIN_DIR" SEARCH_BIN=rg SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" /bin/bash "$SCRIPT"
else
  echo "SKIP: rg-only environment is supported (rg missing)"
fi

if command -v grep >/dev/null 2>&1; then
  ln -s "$(command -v grep)" "$TMP_GREP_BIN_DIR/grep"
  ln -s "$(command -v awk)" "$TMP_GREP_BIN_DIR/awk"
  ln -s "$(command -v tr)" "$TMP_GREP_BIN_DIR/tr"

  run_expect_success "grep-only environment is supported" \
    env PATH="$TMP_GREP_BIN_DIR" SEARCH_BIN=grep SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" /bin/bash "$SCRIPT"

  run_expect_success "auto-detect falls back to grep" \
    env PATH="$TMP_GREP_BIN_DIR" SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" /bin/bash "$SCRIPT"
else
  echo "SKIP: grep-only environment is supported (grep missing)"
  echo "SKIP: auto-detect falls back to grep (grep missing)"
fi

echo "h23 contract script tests passed"
