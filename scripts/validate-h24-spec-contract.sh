#!/usr/bin/env bash
set -euo pipefail

# Maintainer note: this validator intentionally enforces a strict, line-oriented
# contract for the H2.4 spec. Failures are usually contract drift in normative
# clauses, not parser bugs. Keep required wording/structure stable unless you
# also update this validator and its mutation tests in the same change.

SPEC_FILE="${SPEC_FILE:-docs/specs/h2-4-model-aware-prompt-tuning-spec.md}"
CONTEXT_FILE="${CONTEXT_FILE:-docs/specs/h2-4-issue-558-context.md}"
VALIDATOR_JSON_OUT="${VALIDATOR_JSON_OUT:-}"
FORCE_NO_RG="${FORCE_NO_RG:-0}"

VERIFIED_CLAUSES=()
SEARCH_BACKEND=""

# Centralized normative constants for section 4.2/4.3 contract checks.
EXPECTED_BASELINE_COMMIT='9af3ce894999'
BASELINE_COMMIT_PATTERN='^\*\*Baseline evidence snapshot commit:\*\* `[0-9a-f]{12}`$'
SECTION_3_1_REQUIRED_BULLETS=(
  '1. Prompt modes as an explicit axis (`full`/`minimal`/`none` concept).'
  '2. Conditional section inclusion as policy, not ad hoc string editing.'
  '3. Runtime metadata injected into prompt for model self-awareness.'
  '4. Treat prompt size as a resource with hard limits and deterministic trimming.'
)
SECTION_3_2_REQUIRED_BULLETS=(
  '1. OpenClaw is session-type first; Citros needs both session-type and model-tier behavior.'
  '2. OpenClaw skill loading is filesystem/plugin oriented; Citros is fixed-tool mobile architecture.'
  '3. Citros must prioritize mobile latency and token cost more aggressively on `SMALL` tier.'
)
SECTION_3_3_REQUIRED_BULLETS=(
  '1. Weakening safety text for smaller models.'
  '2. Adding large plugin/skills complexity into H2 prompt tuning scope.'
  '3. Overfitting to provider-specific quirks in this H2.4 slice.'
)
CONTEXT_REQUIRED_LINES=(
  '# H2.4 Issue Context: Model-Aware Prompt Tuning (#558)'
  '## Problem Statement'
  'H2.4 requires prompt construction behavior that is deterministic and testable across prompt modes (`FULL`, `MINIMAL`, `NONE`), model tiers (`FLAGSHIP`, `STANDARD`, `SMALL`), and accessibility capability state (attached vs detached).'
  '## Required Outcomes'
  '## Constraints'
  '1. Define explicit prompt policy matrix and invariant rules.'
  '2. Quantify prompt budget thresholds and deterministic trim order.'
  '3. Preserve safety guarantees while allowing tier-specific verbosity.'
  '4. Standardize runtime telemetry format for machine parsing and auditing.'
  '5. Define rollout gates and measurable rollback thresholds.'
  '6. Define executable tests that verify all normative constraints.'
  '1. Scope remains H2.4 prompt tuning only (no new tool system redesign).'
  '2. Contracts must be implementation-safe and independently reproducible by reviewers and CI.'
  '3. Safety and confirmation semantics cannot be weakened on smaller models.'
)
MODE_TIER_MATRIX_ROWS=(
  '| `FULL` | Full strategy, detailed tool guidance, full recovery/comms/rules | Same sections, moderate verbosity | Reduced tool/strategy verbosity, same safety constraints |'
  '| `MINIMAL` | Compact execution reminders + safety | Same | Shortest actionable reminders + same safety |'
  '| `NONE` | Identity only, no tools/safety/runtime | Same | Same |'
)
MODE_TIER_MATRIX_RULES=(
  '- `phoneControlAvailable=false` strips actionable phone-tool guidance and injects accessibility warning in `FULL`/`MINIMAL`.'
)
PROMPT_BUDGET_ROWS=(
  '| `FULL` | 2200 | 2600 |'
  '| `MINIMAL` | 900 | 1100 |'
  '| `NONE` | 40 | 60 |'
)
PROMPT_BUDGET_RULE_HEADERS=(
  '2. Deterministically trim to hard budget using this exact order (lowest priority first):'
  '3. Never trim these sections:'
)
PROMPT_BUDGET_TRIM_ORDER=(
  '   1. verbose examples'
  '   2. communication style detail'
  '   3. recovery elaboration'
  '   4. tool parameter detail'
  '   5. strategy detail'
)
PROMPT_BUDGET_NON_TRIMMABLE=(
  '   1. identity baseline'
  '   2. security block'
  '   3. critical execution rules (`type_text` does not submit, stale-ID warning)'
  '   4. capability warning when accessibility is unavailable'
  '   5. runtime line (except in `NONE`)'
)

has_line_in_file() {
  local file="$1"
  local needle="$2"
  if [[ "$SEARCH_BACKEND" == "rg" ]]; then
    rg -Fq -- "$needle" "$file"
  else
    grep -Fq -- "$needle" "$file"
  fi
}

select_search_backend() {
  case "$FORCE_NO_RG" in
    1|true|TRUE|yes|YES|on|ON)
      SEARCH_BACKEND="grep"
      ;;
    *)
      if command -v rg >/dev/null 2>&1; then
        SEARCH_BACKEND="rg"
      else
        SEARCH_BACKEND="grep"
      fi
      ;;
  esac
}

has_line() {
  local needle="$1"
  has_line_in_file "$SPEC_FILE" "$needle"
}

record_verified() {
  VERIFIED_CLAUSES+=("$1")
}

require_line() {
  local needle="$1"
  local label="$2"

  if ! has_line "$needle"; then
    echo "missing required line: $needle" >&2
    exit 1
  fi

  record_verified "$label"
}

require_pattern() {
  local pattern="$1"
  local error_message="$2"
  local label="$3"

  if ! grep -Eq -- "$pattern" "$SPEC_FILE"; then
    echo "$error_message" >&2
    exit 1
  fi

  record_verified "$label"
}

require_context_line() {
  local needle="$1"
  local label="$2"

  if ! has_line_in_file "$CONTEXT_FILE" "$needle"; then
    echo "context semantic check failed: missing required line: $needle" >&2
    exit 1
  fi

  record_verified "$label"
}

require_exact_baseline_commit() {
  local expected_line="**Baseline evidence snapshot commit:** \`$EXPECTED_BASELINE_COMMIT\`"
  local actual_line

  actual_line="$(grep -E '^\*\*Baseline evidence snapshot commit:\*\* `[^`]+`$' "$SPEC_FILE" | head -n 1 || true)"
  if [[ -z "$actual_line" ]]; then
    echo "baseline evidence snapshot commit format mismatch" >&2
    exit 1
  fi

  if [[ "$actual_line" != "$expected_line" ]]; then
    echo "baseline evidence snapshot commit mismatch: expected $EXPECTED_BASELINE_COMMIT" >&2
    exit 1
  fi

  record_verified "baseline commit exact pin"
}

require_invariant() {
  local invariant_id="$1"
  if ! has_line "- \`$invariant_id\`:"; then
    echo "missing required invariant: $invariant_id" >&2
    exit 1
  fi

  record_verified "invariant $invariant_id"
}

require_rollback_clause() {
  local clause="$1"
  if ! has_line "$clause"; then
    echo "missing rollback trigger clause: $clause" >&2
    exit 1
  fi

  record_verified "rollback clause: $clause"
}

require_mapping_entry() {
  local invariant_id="$1"
  local test_id="$2"
  local section="$3"
  local row

  row="$(awk -v invariant="$invariant_id" '$0 ~ "^\\| " invariant " \\|" { print; exit }' <<<"$section")"
  if [[ -z "$row" || "$row" != *"\`$test_id\`"* ]]; then
    echo "missing required mapping entry: $invariant_id -> $test_id" >&2
    exit 1
  fi

  record_verified "mapping $invariant_id -> $test_id"
}

require_runtime_schema() {
  local expected='`runtime|ts=<RFC3339 UTC>|model=<model_name>|tier=<FLAGSHIP|STANDARD|SMALL>|mode=<FULL|MINIMAL|NONE>|accessibility=<attached|detached>|tool_policy=<policy_id>|prompt_chars=<int>|prompt_tokens_est=<int>|trimmed=<true|false>|trimmed_sections=<comma_list_or_none>`'

  if ! has_line "$expected"; then
    echo "runtime schema mismatch" >&2
    exit 1
  fi

  record_verified "runtime schema"
}

store_section() {
  local var_name="$1"
  local start_heading="$2"
  local end_heading="$3"
  local section

  if [[ -n "$end_heading" ]]; then
    section="$(awk -v start="$start_heading" -v end="$end_heading" '
      $0 == start { in_section = 1; seen = 1; next }
      $0 == end && in_section { exit }
      in_section { print }
      END { if (!seen) exit 2 }
    ' "$SPEC_FILE")" || {
      echo "missing section heading: $start_heading" >&2
      exit 1
    }
  else
    section="$(awk -v start="$start_heading" '
      $0 == start { in_section = 1; seen = 1; next }
      in_section && /^## / { exit }
      in_section { print }
      END { if (!seen) exit 2 }
    ' "$SPEC_FILE")" || {
      echo "missing section heading: $start_heading" >&2
      exit 1
    }
  fi

  printf -v "$var_name" '%s' "$section"
}

require_line_in_section() {
  local var_name="$1"
  local needle="$2"
  local error_prefix="$3"
  local label="$4"
  local section_content="${!var_name}"

  if ! grep -Fq -- "$needle" <<<"$section_content"; then
    echo "$error_prefix: $needle" >&2
    exit 1
  fi

  record_verified "$label"
}

require_pattern_in_section() {
  local var_name="$1"
  local pattern="$2"
  local error_prefix="$3"
  local label="$4"
  local section_content="${!var_name}"

  if ! grep -Eq -- "$pattern" <<<"$section_content"; then
    echo "$error_prefix" >&2
    exit 1
  fi

  record_verified "$label"
}

escape_for_ere() {
  sed -E 's/[][(){}.^$*+?|\\]/\\&/g' <<<"$1"
}

require_safety_clause() {
  local safety_id="$1"
  local clause_text="$2"
  local base_text="$clause_text"
  local escaped

  case "$base_text" in
    *"."|*";"|*"!"|*"?")
      base_text="${base_text%?}"
      ;;
  esac

  escaped="$(escape_for_ere "$base_text")"
  require_pattern_in_section \
    "SECTION_CANONICAL_SAFETY" \
    "^- \`$safety_id\`: \"${escaped}[.;!?]\"$" \
    "missing canonical safety clause: $safety_id" \
    "canonical safety clause $safety_id"
}

require_section_test_id() {
  local key="$1"
  local test_id="$2"
  local section_name="$3"
  require_line_in_section \
    "$key" \
    "\`$test_id\`:" \
    "missing required test id in section: $section_name" \
    "test id $test_id in $section_name"
}

if [[ ! -f "$SPEC_FILE" ]]; then
  echo "missing spec file: $SPEC_FILE" >&2
  exit 1
fi

if [[ ! -f "$CONTEXT_FILE" ]]; then
  echo "missing issue context file: $CONTEXT_FILE" >&2
  exit 1
fi

select_search_backend

require_line '**Issue context source:** `docs/specs/h2-4-issue-558-context.md`' "issue context reference"
for i in "${!CONTEXT_REQUIRED_LINES[@]}"; do
  require_context_line \
    "${CONTEXT_REQUIRED_LINES[$i]}" \
    "context semantic line $((i + 1))"
done
require_pattern "$BASELINE_COMMIT_PATTERN" "baseline evidence snapshot commit format mismatch" "baseline commit format"
require_exact_baseline_commit

require_line "### 3.1 What to Copy" "openclaw comparison section"
require_line "### 3.2 What to Adapt (Citros-Specific)" "openclaw adapt section"
require_line "### 3.3 What Not to Copy" "openclaw do-not-copy section"
require_line "### 4.1 Required Invariants" "required invariants section"
require_line "### 4.2 Mode x Tier Matrix" "mode-tier matrix section"
require_line "### 4.3 Prompt Budget Policy" "prompt budget policy section"

store_section "SECTION_OPENCLAW_ADAPT" "### 3.2 What to Adapt (Citros-Specific)" "### 3.3 What Not to Copy"
store_section "SECTION_OPENCLAW_COPY" "### 3.1 What to Copy" "### 3.2 What to Adapt (Citros-Specific)"
store_section "SECTION_OPENCLAW_NOT_COPY" "### 3.3 What Not to Copy" "## 4. Proposed Prompt Policy Contract"
for i in "${!SECTION_3_1_REQUIRED_BULLETS[@]}"; do
  require_line_in_section \
    "SECTION_OPENCLAW_COPY" \
    "${SECTION_3_1_REQUIRED_BULLETS[$i]}" \
    "missing section 3.1 required bullet" \
    "section 3.1 required bullet $((i + 1))"
done
for i in "${!SECTION_3_2_REQUIRED_BULLETS[@]}"; do
  require_line_in_section \
    "SECTION_OPENCLAW_ADAPT" \
    "${SECTION_3_2_REQUIRED_BULLETS[$i]}" \
    "missing section 3.2 required bullet" \
    "section 3.2 required bullet $((i + 1))"
done
for i in "${!SECTION_3_3_REQUIRED_BULLETS[@]}"; do
  require_line_in_section \
    "SECTION_OPENCLAW_NOT_COPY" \
    "${SECTION_3_3_REQUIRED_BULLETS[$i]}" \
    "missing section 3.3 required bullet" \
    "section 3.3 required bullet $((i + 1))"
done

store_section "SECTION_MODE_TIER_MATRIX" "### 4.2 Mode x Tier Matrix" "### 4.3 Prompt Budget Policy"
for i in "${!MODE_TIER_MATRIX_ROWS[@]}"; do
  require_line_in_section \
    "SECTION_MODE_TIER_MATRIX" \
    "${MODE_TIER_MATRIX_ROWS[$i]}" \
    "missing mode-tier matrix row" \
    "mode-tier matrix row $((i + 1))"
done

for i in "${!MODE_TIER_MATRIX_RULES[@]}"; do
  require_line_in_section \
    "SECTION_MODE_TIER_MATRIX" \
    "${MODE_TIER_MATRIX_RULES[$i]}" \
    "missing mode-tier matrix rule" \
    "mode-tier matrix rule $((i + 1))"
done

store_section "SECTION_PROMPT_BUDGET" "### 4.3 Prompt Budget Policy" "### 4.4 Canonical Safety Text Contract"
require_line_in_section \
  "SECTION_PROMPT_BUDGET" \
  'Token estimate method: `estimated_tokens = ceil(utf8_char_count / 4)`' \
  "missing prompt budget token estimate method" \
  "token estimate method"

for i in "${!PROMPT_BUDGET_ROWS[@]}"; do
  require_line_in_section \
    "SECTION_PROMPT_BUDGET" \
    "${PROMPT_BUDGET_ROWS[$i]}" \
    "missing budget table row" \
    "budget table row $((i + 1))"
done

for i in "${!PROMPT_BUDGET_RULE_HEADERS[@]}"; do
  require_line_in_section \
    "SECTION_PROMPT_BUDGET" \
    "${PROMPT_BUDGET_RULE_HEADERS[$i]}" \
    "missing prompt budget rule header" \
    "prompt budget rule header $((i + 1))"
done

for i in "${!PROMPT_BUDGET_TRIM_ORDER[@]}"; do
  require_line_in_section \
    "SECTION_PROMPT_BUDGET" \
    "${PROMPT_BUDGET_TRIM_ORDER[$i]}" \
    "missing trim-order rule" \
    "trim order $((i + 1))"
done

for i in "${!PROMPT_BUDGET_NON_TRIMMABLE[@]}"; do
  require_line_in_section \
    "SECTION_PROMPT_BUDGET" \
    "${PROMPT_BUDGET_NON_TRIMMABLE[$i]}" \
    "missing non-trimmable rule" \
    "non-trimmable $((i + 1))"
done

require_line "### 4.4 Canonical Safety Text Contract" "canonical safety contract section"

store_section "SECTION_CANONICAL_SAFETY" "### 4.4 Canonical Safety Text Contract" "### 4.5 Runtime Line Schema"
require_line_in_section "SECTION_CANONICAL_SAFETY" "Canonical safety clauses:" "missing canonical safety contract header" "canonical safety clauses header"
require_safety_clause "SAFE-001" "Never perform irreversible or high-stakes user actions without explicit confirmation."
require_safety_clause "SAFE-002" "If tool output is ambiguous, stale, or missing required identifiers, request clarification before acting."
require_safety_clause "SAFE-003" "Do not claim task completion unless the required UI state or tool result confirms completion."
require_safety_clause "SAFE-004" "When accessibility control is detached, report the limitation and avoid action instructions that require detached capabilities."
require_line_in_section "SECTION_CANONICAL_SAFETY" "1. Replace repeated whitespace with a single space." "missing allowed shortening rule" "allowed shortening rule 1"
require_line_in_section "SECTION_CANONICAL_SAFETY" "2. Remove parenthetical clarifiers that do not alter modal verbs (\`must\`, \`must not\`, \`never\`, \`do not\`)." "missing allowed shortening rule" "allowed shortening rule 2"
require_line_in_section "SECTION_CANONICAL_SAFETY" "3. Convert punctuation style (\`;\` vs \`.\`) without removing obligations/prohibitions." "missing allowed shortening rule" "allowed shortening rule 3"
require_line_in_section "SECTION_CANONICAL_SAFETY" "1. Removing negations (\`not\`, \`never\`, \`do not\`)." "missing disallowed shortening rule" "disallowed shortening rule 1"
require_line_in_section "SECTION_CANONICAL_SAFETY" "2. Removing confirmation requirements." "missing disallowed shortening rule" "disallowed shortening rule 2"
require_line_in_section "SECTION_CANONICAL_SAFETY" "3. Removing stale/ambiguous-output safety checks." "missing disallowed shortening rule" "disallowed shortening rule 3"

store_section "SECTION_RUNTIME_SCHEMA" "### 4.5 Runtime Line Schema" "## 5. Failure Modes and Guardrails"
require_runtime_schema
require_line_in_section "SECTION_RUNTIME_SCHEMA" "   - \`model\` may include provider model ID only." "missing runtime redaction requirement" "runtime redaction: model provider id only"
require_line_in_section "SECTION_RUNTIME_SCHEMA" "   - Do not include user content, tool arguments, contact names, or message text in runtime line." "missing runtime redaction requirement" "runtime redaction: no user/tool/contact/message text"
require_line_in_section "SECTION_RUNTIME_SCHEMA" "   - \`trimmed_sections\` may include section IDs only (not section content)." "missing runtime redaction requirement" "runtime redaction: trimmed_sections ids only"
require_line '5. `trimmed_sections` must use canonical section IDs sorted lexicographically ascending; use `none` when no sections were trimmed.' "trimmed_sections canonicalization"

require_invariant "INV-001"
require_invariant "INV-002"
require_invariant "INV-003"
require_invariant "INV-004"
require_invariant "INV-005"
require_invariant "INV-006"
require_invariant "INV-007"

require_line "### 6.4 Concurrency Tests" "concurrency tests section"
store_section "SECTION_UNIT_TESTS" "### 6.1 Unit Tests" "### 6.2 Integration Tests"
store_section "SECTION_INTEGRATION_TESTS" "### 6.2 Integration Tests" "### 6.3 Manual / Device Validation"
store_section "SECTION_CONCURRENCY_TESTS" "### 6.4 Concurrency Tests" "### 6.5 Invariant-to-Test Mapping"
store_section "SECTION_INVARIANT_MAPPING" "### 6.5 Invariant-to-Test Mapping" "## 7. Rollout Plan"

require_section_test_id "SECTION_UNIT_TESTS" "UT-H24-001" "6.1 Unit Tests"
require_section_test_id "SECTION_UNIT_TESTS" "UT-H24-002" "6.1 Unit Tests"
require_section_test_id "SECTION_UNIT_TESTS" "UT-H24-003" "6.1 Unit Tests"
require_section_test_id "SECTION_UNIT_TESTS" "UT-H24-004" "6.1 Unit Tests"
require_section_test_id "SECTION_UNIT_TESTS" "UT-H24-005" "6.1 Unit Tests"
require_section_test_id "SECTION_UNIT_TESTS" "UT-H24-006" "6.1 Unit Tests"
require_section_test_id "SECTION_INTEGRATION_TESTS" "IT-H24-001" "6.2 Integration Tests"
require_section_test_id "SECTION_INTEGRATION_TESTS" "IT-H24-002" "6.2 Integration Tests"
require_section_test_id "SECTION_INTEGRATION_TESTS" "IT-H24-003" "6.2 Integration Tests"
require_section_test_id "SECTION_CONCURRENCY_TESTS" "CT-H24-001" "6.4 Concurrency Tests"
require_section_test_id "SECTION_CONCURRENCY_TESTS" "CT-H24-002" "6.4 Concurrency Tests"

require_line "### 7.4 Rollback Triggers" "rollback triggers section"
require_rollback_clause '1. tool-loop failure or abandonment rate increases by `>= 5.0%` absolute vs pre-rollout baseline.'
require_rollback_clause '2. any confirmed safety or confirmation-policy regression (`>= 1` verified event).'
require_rollback_clause '3. wrong-mode production events (`NONE` used in tool-capable turn) exceeds `0.1%` of tool-capable turns.'
require_rollback_clause '4. p95 latency for `SMALL` tier increases by `>= 20%` with no corresponding completion-rate gain (`< 1.0%` absolute).'

require_mapping_entry "INV-001" "UT-H24-001" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-001" "UT-H24-002" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-002" "UT-H24-002" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-003" "UT-H24-005" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-003" "IT-H24-001" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-004" "UT-H24-004" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-004" "CT-H24-002" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-005" "UT-H24-006" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-005" "IT-H24-003" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-006" "UT-H24-001" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-006" "IT-H24-002" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-007" "CT-H24-001" "$SECTION_INVARIANT_MAPPING"
require_mapping_entry "INV-007" "CT-H24-002" "$SECTION_INVARIANT_MAPPING"

require_line "## 10. Glossary" "glossary section"

echo "verified clauses:"
for clause in "${VERIFIED_CLAUSES[@]}"; do
  echo "- $clause"
done

echo "h2.4 spec contract validation passed"
echo "validator_status=pass"
echo "validator_spec=h2.4"
echo "validator_search_backend=$SEARCH_BACKEND"
echo "verified_clause_count=${#VERIFIED_CLAUSES[@]}"

json_clauses=''
for clause in "${VERIFIED_CLAUSES[@]}"; do
  escaped_clause="$(sed 's/\\/\\\\/g; s/"/\\"/g' <<<"$clause")"
  if [[ -z "$json_clauses" ]]; then
    json_clauses="\"$escaped_clause\""
  else
    json_clauses="$json_clauses,\"$escaped_clause\""
  fi
done

validator_summary_json="{\"status\":\"pass\",\"spec\":\"h2.4\",\"verified_clause_count\":${#VERIFIED_CLAUSES[@]},\"verified_clauses\":[${json_clauses}]}"
echo "validator_summary_json=$validator_summary_json"

if [[ -n "$VALIDATOR_JSON_OUT" ]]; then
  printf '%s\n' "$validator_summary_json" >"$VALIDATOR_JSON_OUT"
fi
