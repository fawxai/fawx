#!/usr/bin/env bash
# shellcheck disable=SC2016  # intentional single-quoted regex/jq snippets in spec checks
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VALIDATOR="$ROOT_DIR/scripts/validate-h24-spec-contract.sh"
SOURCE_SPEC="$ROOT_DIR/docs/specs/h2-4-model-aware-prompt-tuning-spec.md"
SOURCE_CONTEXT="$ROOT_DIR/docs/specs/h2-4-issue-558-context.md"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

sed_inplace() {
  local expr="$1"
  local file="$2"
  if sed --version >/dev/null 2>&1; then
    sed -i -e "$expr" "$file"
  else
    sed -i '' -e "$expr" "$file"
  fi
}

run_validator() {
  local spec_file="$1"
  local context_file="${2:-$SOURCE_CONTEXT}"
  shift $(( $# > 1 ? 2 : 1 ))

  SPEC_FILE="$spec_file" CONTEXT_FILE="$context_file" "$@" "$VALIDATOR"
}

run_validator_without_rg() {
  local spec_file="$1"
  local context_file="${2:-$SOURCE_CONTEXT}"
  FORCE_NO_RG=1 SPEC_FILE="$spec_file" CONTEXT_FILE="$context_file" "$VALIDATOR"
}

run_validator_with_json_artifact() {
  local spec_file="$1"
  local json_out="$2"
  SPEC_FILE="$spec_file" CONTEXT_FILE="$SOURCE_CONTEXT" VALIDATOR_JSON_OUT="$json_out" "$VALIDATOR"
}

extract_verified_clause_count() {
  local output_file="$1"
  awk -F= '/^verified_clause_count=/{print $2}' "$output_file" | tail -n 1
}

assert_pass() {
  local name="$1"
  shift
  local output_file
  output_file="$(mktemp)"

  if ! "$@" >"$output_file" 2>&1; then
    cat "$output_file" >&2
    rm -f "$output_file"
    fail "$name"
  fi

  rm -f "$output_file"
}

assert_pass_contains() {
  local name="$1"
  local expected="$2"
  shift 2
  local output_file
  output_file="$(mktemp)"

  if ! "$@" >"$output_file" 2>&1; then
    cat "$output_file" >&2
    rm -f "$output_file"
    fail "$name"
  fi

  if ! grep -Fq "$expected" "$output_file"; then
    cat "$output_file" >&2
    rm -f "$output_file"
    fail "$name (missing expected text: $expected)"
  fi

  rm -f "$output_file"
}

assert_default_backend_marker() {
  local output_file="$1"
  local expected_backend="grep"
  case "${FORCE_NO_RG:-0}" in
    1|true|TRUE|yes|YES|on|ON)
      expected_backend="grep"
      ;;
    *)
      if command -v rg >/dev/null 2>&1; then
        expected_backend="rg"
      fi
      ;;
  esac

  if ! grep -Fq "validator_search_backend=$expected_backend" "$output_file"; then
    cat "$output_file" >&2
    fail "missing expected default backend marker: $expected_backend"
  fi
}

assert_forced_grep_backend() {
  local output_file="$1"
  if ! grep -Fq "validator_search_backend=grep" "$output_file"; then
    cat "$output_file" >&2
    fail "missing expected forced grep backend marker"
  fi
}

assert_fail_contains() {
  local name="$1"
  local expected="$2"
  shift 2

  local output_file
  output_file="$(mktemp)"

  if "$@" >"$output_file" 2>&1; then
    cat "$output_file" >&2
    rm -f "$output_file"
    fail "$name (unexpected pass)"
  fi

  if ! grep -Fq "$expected" "$output_file"; then
    cat "$output_file" >&2
    rm -f "$output_file"
    fail "$name (missing expected text: $expected)"
  fi

  rm -f "$output_file"
}

make_variant() {
  local name="$1"
  local from="$SOURCE_SPEC"
  local to="$TMP_DIR/$name/spec.md"
  mkdir -p "$(dirname "$to")"
  cp "$from" "$to"

  case "$name" in
    pass)
      ;;
    fail-missing-invariant)
      awk '$0 != "- `INV-007`: Prompt construction and telemetry emission must be thread-safe under concurrent requests."' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-runtime-schema)
      sed_inplace 's/prompt_tokens_est=<int>/prompt_tokens=<int>/' "$to"
      ;;
    fail-rollback-threshold)
      sed_inplace 's/`>= 20%`/`>= 25%`/' "$to"
      ;;
    fail-mapping)
      sed_inplace 's/`UT-H24-004`, `CT-H24-002`/`UT-H24-004`/' "$to"
      ;;
    pass-prose-variation)
      awk '
        $0 == "- `SAFE-001`: \"Never perform irreversible or high-stakes user actions without explicit confirmation.\"" {
          print "- `SAFE-001`: \"Never perform irreversible or high-stakes user actions without explicit confirmation;\"";
          next;
        }
        $0 == "- `SAFE-002`: \"If tool output is ambiguous, stale, or missing required identifiers, request clarification before acting.\"" {
          print "- `SAFE-002`: \"If tool output is ambiguous, stale, or missing required identifiers, request clarification before acting!\"";
          next;
        }
        $0 == "- `SAFE-003`: \"Do not claim task completion unless the required UI state or tool result confirms completion.\"" {
          print "- `SAFE-003`: \"Do not claim task completion unless the required UI state or tool result confirms completion?\"";
          next;
        }
        $0 == "- `SAFE-004`: \"When accessibility control is detached, report the limitation and avoid action instructions that require detached capabilities.\"" {
          print "- `SAFE-004`: \"When accessibility control is detached, report the limitation and avoid action instructions that require detached capabilities!\"";
          next;
        }
        { print }
      ' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-safe-clause)
      awk '$0 != "- `SAFE-003`: \"Do not claim task completion unless the required UI state or tool result confirms completion.\""' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-safe-header)
      awk '$0 != "Canonical safety clauses:"' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-redaction-rule)
      awk '$0 != "   - Do not include user content, tool arguments, contact names, or message text in runtime line."' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-redaction-model-rule)
      awk '$0 != "   - `model` may include provider model ID only."' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-redaction-trimmed-sections-rule)
      awk '$0 != "   - `trimmed_sections` may include section IDs only (not section content)."' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-trim-order-rule)
      awk '$0 != "   4. tool parameter detail"' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-non-trimmable-rule)
      awk '$0 != "   2. security block"' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-disallowed-shortening)
      awk '$0 != "3. Removing stale/ambiguous-output safety checks."' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-test-id-wrong-section)
      awk '
        $0 == "- `UT-H24-003`: over-budget fixtures trigger deterministic trimming order." {
          print "- `MT-H24-099`: references `UT-H24-003` outside required section for contract test.";
          next;
        }
        { print }
      ' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-baseline-commit-format)
      sed_inplace 's/\*\*Baseline evidence snapshot commit:\*\* `9af3ce894999`/**Baseline evidence snapshot commit:** `9af3ce89499`/' "$to"
      ;;
    fail-baseline-commit-drift)
      sed_inplace 's/\*\*Baseline evidence snapshot commit:\*\* `9af3ce894999`/**Baseline evidence snapshot commit:** `111111111111`/' "$to"
      ;;
    fail-missing-adapt-heading)
      awk '$0 != "### 3.2 What to Adapt (Fawx-Specific)"' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-do-not-copy-heading)
      awk '$0 != "### 3.3 What Not to Copy"' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-adapt-bullet)
      awk '$0 != "1. OpenClaw is session-type first; Fawx needs both session-type and model-tier behavior."' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    fail-missing-do-not-copy-bullet)
      awk '$0 != "1. Weakening safety text for smaller models."' "$to" >"$to.tmp" && mv "$to.tmp" "$to"
      ;;
    copy-*|adapt-*|do-not-copy-*|context-*|matrix-*|budget-*)
      ;;
    *)
      fail "unknown variant: $name"
      ;;
  esac

  printf '%s\n' "$to"
}

make_context_variant() {
  local name="$1"
  local from="$SOURCE_CONTEXT"
  local to="$TMP_DIR/$name/context.md"
  mkdir -p "$(dirname "$to")"
  cp "$from" "$to"
  printf '%s\n' "$to"
}

replace_exact_line() {
  local file="$1"
  local old="$2"
  local new="$3"
  local replaced_file="$file.tmp.replaced"
  local replacements

  awk -v old="$old" -v new="$new" '
    $0 == old {
      print new
      replaced += 1
      next
    }
    { print }
    END {
      if (replaced == 0) {
        exit 2
      }
      printf "%d\n", replaced > "/dev/stderr"
    }
  ' "$file" >"$replaced_file" 2>"$replaced_file.count" || fail "failed to replace line in mutation"

  replacements="$(tr -d '[:space:]' <"$replaced_file.count")"
  rm -f "$replaced_file.count"
  if [[ "$replacements" != "1" ]]; then
    rm -f "$replaced_file"
    fail "expected exactly one replacement for mutation, got: $replacements"
  fi

  mv "$replaced_file" "$file"
}

run_normative_mutation_failure_suite() {
  local variant_name
  local expected_error
  local old_line
  local new_line
  local spec_file

  while IFS=$'\t' read -r variant_name expected_error old_line new_line; do
    [[ -z "$variant_name" ]] && continue
    spec_file="$(make_variant "$variant_name")"
    replace_exact_line "$spec_file" "$old_line" "$new_line"
    assert_fail_contains \
      "$variant_name is rejected" \
      "$expected_error" \
      run_validator "$spec_file"
  done <<'EOF'
copy-bullet-1	missing section 3.1 required bullet	1. Prompt modes as an explicit axis (`full`/`minimal`/`none` concept).	1. Prompt modes as an explicit axis (`full`/`minimal` concept).
copy-bullet-2	missing section 3.1 required bullet	2. Conditional section inclusion as policy, not ad hoc string editing.	2. Conditional section inclusion as policy.
copy-bullet-3	missing section 3.1 required bullet	3. Runtime metadata injected into prompt for model self-awareness.	3. Runtime metadata injected for model self-awareness.
copy-bullet-4	missing section 3.1 required bullet	4. Treat prompt size as a resource with hard limits and deterministic trimming.	4. Treat prompt size as a resource with hard limits.
adapt-bullet-1	missing section 3.2 required bullet	1. OpenClaw is session-type first; Fawx needs both session-type and model-tier behavior.	1. OpenClaw is session-type first; Fawx needs model-tier behavior.
adapt-bullet-2	missing section 3.2 required bullet	2. OpenClaw skill loading is filesystem/plugin oriented; Fawx is fixed-tool mobile architecture.	2. OpenClaw skill loading is plugin oriented; Fawx is mobile architecture.
adapt-bullet-3	missing section 3.2 required bullet	3. Fawx must prioritize mobile latency and token cost more aggressively on `SMALL` tier.	3. Fawx must prioritize latency on `SMALL` tier.
do-not-copy-bullet-1	missing section 3.3 required bullet	1. Weakening safety text for smaller models.	1. Weakening safety text.
do-not-copy-bullet-2	missing section 3.3 required bullet	2. Adding large plugin/skills complexity into H2 prompt tuning scope.	2. Adding plugin complexity into H2 scope.
do-not-copy-bullet-3	missing section 3.3 required bullet	3. Overfitting to provider-specific quirks in this H2.4 slice.	3. Overfitting to provider quirks in this H2.4 slice.
matrix-full-row	missing mode-tier matrix row	| `FULL` | Full strategy, detailed tool guidance, full recovery/comms/rules | Same sections, moderate verbosity | Reduced tool/strategy verbosity, same safety constraints |	| `FULL` | Full strategy, detailed tool guidance, full recovery/comms/rules | Same sections, moderate verbosity | Reduced tool/strategy verbosity |
matrix-minimal-row	missing mode-tier matrix row	| `MINIMAL` | Compact execution reminders + safety | Same | Shortest actionable reminders + same safety |	| `MINIMAL` | Compact execution reminders + safety | Same | Shortest actionable reminders |
matrix-none-row	missing mode-tier matrix row	| `NONE` | Identity only, no tools/safety/runtime | Same | Same |	| `NONE` | Identity only | Same | Same |
matrix-accessibility-rule	missing mode-tier matrix rule	- `phoneControlAvailable=false` strips actionable phone-tool guidance and injects accessibility warning in `FULL`/`MINIMAL`.	- `phoneControlAvailable=false` strips actionable phone-tool guidance in `FULL`/`MINIMAL`.
budget-full-row	missing budget table row	| `FULL` | 2200 | 2600 |	| `FULL` | 2200 | 2500 |
budget-minimal-row	missing budget table row	| `MINIMAL` | 900 | 1100 |	| `MINIMAL` | 950 | 1100 |
budget-none-row	missing budget table row	| `NONE` | 40 | 60 |	| `NONE` | 40 | 70 |
EOF
}

run_context_mutation_failure_suite() {
  local variant_name
  local expected_error
  local old_line
  local new_line
  local context_file

  while IFS=$'\t' read -r variant_name expected_error old_line new_line; do
    [[ -z "$variant_name" ]] && continue
    context_file="$(make_context_variant "$variant_name")"
    replace_exact_line "$context_file" "$old_line" "$new_line"
    assert_fail_contains \
      "$variant_name is rejected" \
      "$expected_error" \
      run_validator "$PASS_FILE" "$context_file"
  done <<'EOF'
context-problem-statement	missing required line	H2.4 requires prompt construction behavior that is deterministic and testable across prompt modes (`FULL`, `MINIMAL`, `NONE`), model tiers (`FLAGSHIP`, `STANDARD`, `SMALL`), and accessibility capability state (attached vs detached).	H2.4 requires prompt construction behavior across prompt modes and model tiers.
context-outcome-2	missing required line	2. Quantify prompt budget thresholds and deterministic trim order.	2. Quantify prompt budget thresholds.
context-outcome-3	missing required line	3. Preserve safety guarantees while allowing tier-specific verbosity.	3. Preserve safety guarantees.
context-outcome-4	missing required line	4. Standardize runtime telemetry format for machine parsing and auditing.	4. Standardize runtime telemetry format.
context-outcome-5	missing required line	5. Define rollout gates and measurable rollback thresholds.	5. Define rollout gates.
context-outcome-6	missing required line	6. Define executable tests that verify all normative constraints.	6. Define executable tests.
context-constraint-1	missing required line	1. Scope remains H2.4 prompt tuning only (no new tool system redesign).	1. Scope remains H2.4 prompt tuning only.
context-constraint-2	missing required line	2. Contracts must be implementation-safe and independently reproducible by reviewers and CI.	2. Contracts must be implementation-safe and reproducible.
EOF
}

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

PASS_FILE="$(make_variant pass)"
PROSE_VARIATION_FILE="$(make_variant pass-prose-variation)"
MISSING_INVARIANT_FILE="$(make_variant fail-missing-invariant)"
RUNTIME_SCHEMA_FILE="$(make_variant fail-runtime-schema)"
ROLLBACK_FILE="$(make_variant fail-rollback-threshold)"
MAPPING_FILE="$(make_variant fail-mapping)"
MISSING_SAFE_FILE="$(make_variant fail-missing-safe-clause)"
MISSING_SAFE_HEADER_FILE="$(make_variant fail-missing-safe-header)"
MISSING_REDACTION_FILE="$(make_variant fail-missing-redaction-rule)"
MISSING_REDACTION_MODEL_FILE="$(make_variant fail-missing-redaction-model-rule)"
MISSING_REDACTION_TRIMMED_SECTIONS_FILE="$(make_variant fail-missing-redaction-trimmed-sections-rule)"
MISSING_TRIM_ORDER_FILE="$(make_variant fail-missing-trim-order-rule)"
MISSING_NON_TRIMMABLE_FILE="$(make_variant fail-missing-non-trimmable-rule)"
MISSING_SHORTENING_FILE="$(make_variant fail-missing-disallowed-shortening)"
TEST_ID_WRONG_SECTION_FILE="$(make_variant fail-test-id-wrong-section)"
BASELINE_COMMIT_FORMAT_FILE="$(make_variant fail-baseline-commit-format)"
BASELINE_COMMIT_DRIFT_FILE="$(make_variant fail-baseline-commit-drift)"
MISSING_ADAPT_HEADING_FILE="$(make_variant fail-missing-adapt-heading)"
MISSING_DO_NOT_COPY_HEADING_FILE="$(make_variant fail-missing-do-not-copy-heading)"
MISSING_ADAPT_BULLET_FILE="$(make_variant fail-missing-adapt-bullet)"
MISSING_DO_NOT_COPY_BULLET_FILE="$(make_variant fail-missing-do-not-copy-bullet)"

assert_pass "pass fixture validates" run_validator "$PASS_FILE"
PASS_OUTPUT_FILE="$(mktemp)"
run_validator "$PASS_FILE" >"$PASS_OUTPUT_FILE"
assert_default_backend_marker "$PASS_OUTPUT_FILE"
assert_pass_contains \
  "validator emits machine-readable pass status" \
  "validator_status=pass" \
  grep -F "validator_status=pass" "$PASS_OUTPUT_FILE"
assert_pass_contains \
  "validator emits machine-readable json summary" \
  "validator_summary_json={\"status\":\"pass\",\"spec\":\"h2.4\"" \
  grep -F "validator_summary_json=" "$PASS_OUTPUT_FILE"
ACTUAL_CLAUSE_COUNT="$(extract_verified_clause_count "$PASS_OUTPUT_FILE")"
if [[ -z "$ACTUAL_CLAUSE_COUNT" ]]; then
  rm -f "$PASS_OUTPUT_FILE"
  fail "missing verified_clause_count output"
fi
if [[ "$ACTUAL_CLAUSE_COUNT" != "108" ]]; then
  rm -f "$PASS_OUTPUT_FILE"
  fail "verified clause count drifted: expected 108, got $ACTUAL_CLAUSE_COUNT"
fi
rm -f "$PASS_OUTPUT_FILE"
assert_pass \
  "semantic prose punctuation variation still validates" \
  run_validator "$PROSE_VARIATION_FILE"
JSON_ARTIFACT_FILE="$(mktemp)"
assert_pass \
  "validator writes json artifact when requested" \
  run_validator_with_json_artifact "$PASS_FILE" "$JSON_ARTIFACT_FILE"
if ! grep -Fq '"status":"pass"' "$JSON_ARTIFACT_FILE"; then
  rm -f "$JSON_ARTIFACT_FILE"
  fail "json artifact missing pass status"
fi
if ! grep -Fq '"verified_clause_count":108' "$JSON_ARTIFACT_FILE"; then
  rm -f "$JSON_ARTIFACT_FILE"
  fail "json artifact missing stable verified clause count"
fi
rm -f "$JSON_ARTIFACT_FILE"
assert_fail_contains \
  "missing invariant is rejected" \
  "missing required invariant: INV-007" \
  run_validator "$MISSING_INVARIANT_FILE"
assert_fail_contains \
  "runtime schema mismatch is rejected" \
  "runtime schema mismatch" \
  run_validator "$RUNTIME_SCHEMA_FILE"
assert_fail_contains \
  "rollback threshold mismatch is rejected" \
  "missing rollback trigger clause" \
  run_validator "$ROLLBACK_FILE"
assert_fail_contains \
  "invariant mapping gaps are rejected" \
  "missing required mapping entry: INV-004 -> CT-H24-002" \
  run_validator "$MAPPING_FILE"
assert_fail_contains \
  "missing safety clause is rejected" \
  "missing canonical safety clause: SAFE-003" \
  run_validator "$MISSING_SAFE_FILE"
assert_fail_contains \
  "missing canonical safety header is rejected" \
  "missing canonical safety contract header" \
  run_validator "$MISSING_SAFE_HEADER_FILE"
assert_fail_contains \
  "missing redaction rule is rejected" \
  "missing runtime redaction requirement" \
  run_validator "$MISSING_REDACTION_FILE"
assert_fail_contains \
  "missing runtime redaction model rule is rejected" \
  "missing runtime redaction requirement" \
  run_validator "$MISSING_REDACTION_MODEL_FILE"
assert_fail_contains \
  "missing runtime redaction trimmed_sections rule is rejected" \
  "missing runtime redaction requirement" \
  run_validator "$MISSING_REDACTION_TRIMMED_SECTIONS_FILE"
assert_fail_contains \
  "missing trim-order rule is rejected" \
  "missing trim-order rule" \
  run_validator "$MISSING_TRIM_ORDER_FILE"
assert_fail_contains \
  "missing non-trimmable rule is rejected" \
  "missing non-trimmable rule" \
  run_validator "$MISSING_NON_TRIMMABLE_FILE"
assert_fail_contains \
  "missing disallowed shortening rule is rejected" \
  "missing disallowed shortening rule" \
  run_validator "$MISSING_SHORTENING_FILE"
assert_fail_contains \
  "test IDs outside required sections are rejected" \
  "missing required test id in section" \
  run_validator "$TEST_ID_WRONG_SECTION_FILE"
assert_fail_contains \
  "baseline commit must remain 12 lowercase hex chars" \
  "snapshot commit" \
  run_validator "$BASELINE_COMMIT_FORMAT_FILE"
assert_fail_contains \
  "baseline commit hash drift is rejected" \
  "baseline evidence snapshot commit mismatch" \
  run_validator "$BASELINE_COMMIT_DRIFT_FILE"
assert_fail_contains \
  "missing adapt heading is rejected" \
  "missing required line: ### 3.2 What to Adapt (Fawx-Specific)" \
  run_validator "$MISSING_ADAPT_HEADING_FILE"
assert_fail_contains \
  "missing do-not-copy heading is rejected" \
  "missing required line: ### 3.3 What Not to Copy" \
  run_validator "$MISSING_DO_NOT_COPY_HEADING_FILE"
assert_fail_contains \
  "missing adapt bullet is rejected" \
  "missing section 3.2 required bullet" \
  run_validator "$MISSING_ADAPT_BULLET_FILE"
assert_fail_contains \
  "missing do-not-copy bullet is rejected" \
  "missing section 3.3 required bullet" \
  run_validator "$MISSING_DO_NOT_COPY_BULLET_FILE"
assert_fail_contains \
  "missing context file is rejected" \
  "missing issue context file" \
  run_validator "$PASS_FILE" "$TMP_DIR/nope.md"
assert_fail_contains \
  "semantically invalid context content is rejected" \
  "context semantic check failed" \
  run_validator "$PASS_FILE" "$ROOT_DIR/scripts/tests/fixtures/h24-spec-contract/context.md"

run_normative_mutation_failure_suite
run_context_mutation_failure_suite

assert_pass \
  "validator works when rg backend is forced off" \
  run_validator_without_rg "$PASS_FILE"
FORCED_OUTPUT_FILE="$(mktemp)"
run_validator_without_rg "$PASS_FILE" >"$FORCED_OUTPUT_FILE"
assert_forced_grep_backend "$FORCED_OUTPUT_FILE"
rm -f "$FORCED_OUTPUT_FILE"

echo "All validator tests passed"
