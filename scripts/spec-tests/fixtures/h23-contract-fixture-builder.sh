#!/usr/bin/env bash
set -euo pipefail

write_common_policy_block() {
  cat <<'EOF'
## 5.2 Policy
ResolvedToolPlan
toolNames
reasonCodes
estimatedToolCount
activeCategories: List<ToolCategory>
Resolver state model (normative)
V1 is stateless
Mixed granularity semantics
partial tool pruning
Policy resolution pseudocode (normative)
Action-oriented fallback trigger (normative)
fallback_action_intent
Locale.ROOT
whole-word
final_categories SUBSET_OF policy_allow_set
Fallback cannot reintroduce a category blocked by security, capability, or user disable
if active_set == {CORE} and action_oriented_trigger(message, resolver_signal):
for c in [NAVIGATION, INTERACTION, OBSERVATION]:
if c in allow_set: active_set += c
active_set += CORE
active_ordered = ordered_categories(active_set)
Concurrency/thread-safety
open weather app
find weather online
ResolvedToolPlan examples (normative)
ReasonCode (typed enum)
user_disabled_navigation
user_disabled_research
no raw user message content
Unknown future `reasonCodes` must be ignored by clients (forward compatibility).
EOF
}

write_common_examples_block() {
  local include_unknown_reason_codes_line="${1:-yes}"
  cat <<'EOF'
1. Example A
   ```json
   {
     "activeCategories": ["CORE", "NAVIGATION", "INTERACTION", "OBSERVATION"],
     "reasonCodes": [
       "tier_small_blocks_research",
       "fallback_action_intent"
     ]
   }
   ```
2. Example B
   ```json
   {
     "activeCategories": ["CORE", "INTERACTION", "OBSERVATION"],
     "reasonCodes": [
       "user_disabled_navigation",
       "fallback_action_intent"
     ]
   }
   ```
EOF
  if [[ "$include_unknown_reason_codes_line" == "yes" ]]; then
    echo "Unknown future \`reasonCodes\` must be ignored by clients (forward compatibility)."
  fi
}

build_h23_contract_fixtures() {
  local fixtures_dir="$1"
  mkdir -p "$fixtures_dir"

  cat >"$fixtures_dir/h23-contract-pass.md" <<EOF
# Fixture

$(write_common_policy_block)

### 7.3 Acceptance criteria
Section 9 success criteria gates #1-#5
completion rate
Policy-violation counter

### 7.4 ResolvedToolPlan examples (normative)
$(write_common_examples_block yes)
EOF

  cat >"$fixtures_dir/h23-contract-pass-valid-multi-example-cooccurrence.md" <<EOF
# Fixture

$(write_common_policy_block)

### 7.3 Acceptance criteria
Section 9 success criteria gates #1-#5
completion rate
Policy-violation counter

### 7.4 ResolvedToolPlan examples (normative)
1. Example A
   "activeCategories": ["CORE", "NAVIGATION", "INTERACTION", "OBSERVATION"]
2. Example B
   "reasonCodes": [
     "user_disabled_navigation"
   ]
"activeCategories": [
"reasonCodes": [
"tier_small_blocks_research"
"fallback_action_intent"
Unknown future \`reasonCodes\` must be ignored by clients (forward compatibility).
EOF

  cat >"$fixtures_dir/h23-contract-fail-inconsistent-json-example.md" <<EOF
# Fixture

$(write_common_policy_block)

### 7.3 Acceptance criteria
Section 9 success criteria gates #1-#5
completion rate
Policy-violation counter

### 7.4 ResolvedToolPlan examples (normative)
1. Example A
   "activeCategories": ["CORE", "NAVIGATION", "INTERACTION", "OBSERVATION"]
   "reasonCodes": [
     "user_disabled_navigation",
     "fallback_action_intent"
   ]
"activeCategories": [
"reasonCodes": [
"tier_small_blocks_research"
Unknown future \`reasonCodes\` must be ignored by clients (forward compatibility).
EOF

  cat >"$fixtures_dir/h23-contract-fail-missing-acceptance.md" <<EOF
# Fixture

$(write_common_policy_block)

### 7.3 Acceptance criteria
Section 9 success criteria gates #1-#5
completion rate

### 7.4 ResolvedToolPlan examples (normative)
$(write_common_examples_block yes)
EOF

  cat >"$fixtures_dir/h23-contract-fail-offsection-acceptance.md" <<EOF
# Fixture

$(write_common_policy_block)

## Wrong Section
Section 9 success criteria gates #1-#5
completion rate
Policy-violation counter

### 7.3 Acceptance criteria
This section intentionally omits required gate text.

### 7.4 ResolvedToolPlan examples (normative)
$(write_common_examples_block yes)
EOF

  cat >"$fixtures_dir/h23-contract-fail-missing-unknown-reason-codes.md" <<EOF
# Fixture

$(write_common_policy_block)

### 7.3 Acceptance criteria
Section 9 success criteria gates #1-#5
completion rate
Policy-violation counter

### 7.4 ResolvedToolPlan examples (normative)
$(write_common_examples_block no)
EOF

  cat >"$fixtures_dir/h23-contract-pass-with-subsection.md" <<EOF
# Fixture

$(write_common_policy_block)

### 7.3 Acceptance criteria
#### Operational Gates
Section 9 success criteria gates #1-#5
completion rate
Policy-violation counter

### 7.4 ResolvedToolPlan examples (normative)
$(write_common_examples_block yes)
EOF

  cat >"$fixtures_dir/h23-contract-pass-with-heading-whitespace.md" <<EOF
# Fixture

$(write_common_policy_block)

###   7.3 Acceptance criteria
Section 9 success criteria gates #1-#5
completion rate
Policy-violation counter

###   7.4 ResolvedToolPlan examples (normative)
$(write_common_examples_block yes)
EOF

  cat >"$fixtures_dir/constitution-pass.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
Security, capability, and platform constraints are non-bypassable and must be applied before lower-priority policy layers.
## 5.3 Determinism
Given identical explicit inputs, policy outcomes must be deterministic.
## 7.3 Acceptance Criteria
Spec changes must define measurable acceptance gates and pass them before broad rollout.
## 8.1 Testing Baseline
Behavioral changes require TDD coverage for both pass and fail boundaries.
## 8.6 Privacy Baseline
No raw user message content may be logged in policy telemetry.
## 9. Rollout And Gates
Rollout requires pre-defined promotion gates and rollback criteria.
EOF

  cat >"$fixtures_dir/constitution-fail-missing-8-6.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
## 5.3 Determinism
## 7.3 Acceptance Criteria
## 8.1 Testing Baseline
## 9. Rollout And Gates
EOF

  cat >"$fixtures_dir/constitution-fail-5-2-semantics.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
This text omits precedence and non-bypassable constraints.
## 5.3 Determinism
Given identical explicit inputs, policy outcomes must be deterministic.
## 7.3 Acceptance Criteria
Spec changes must define measurable acceptance gates and pass them before broad rollout.
## 8.1 Testing Baseline
Behavioral changes require TDD coverage for both pass and fail boundaries.
## 8.6 Privacy Baseline
No raw user message content may be logged in policy telemetry.
## 9. Rollout And Gates
Rollout requires pre-defined promotion gates and rollback criteria.
EOF

  cat >"$fixtures_dir/constitution-fail-5-3-semantics.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
Security, capability, and platform constraints are non-bypassable and must be applied before lower-priority policy layers.
## 5.3 Determinism
This text omits deterministic outcome constraints.
## 7.3 Acceptance Criteria
Spec changes must define measurable acceptance gates and pass them before broad rollout.
## 8.1 Testing Baseline
Behavioral changes require TDD coverage for both pass and fail boundaries.
## 8.6 Privacy Baseline
No raw user message content may be logged in policy telemetry.
## 9. Rollout And Gates
Rollout requires pre-defined promotion gates and rollback criteria.
EOF

  cat >"$fixtures_dir/constitution-fail-7-3-semantics.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
Security, capability, and platform constraints are non-bypassable and must be applied before lower-priority policy layers.
## 5.3 Determinism
Given identical explicit inputs, policy outcomes must be deterministic.
## 7.3 Acceptance Criteria
This text omits measurable rollout gates.
## 8.1 Testing Baseline
Behavioral changes require TDD coverage for both pass and fail boundaries.
## 8.6 Privacy Baseline
No raw user message content may be logged in policy telemetry.
## 9. Rollout And Gates
Rollout requires pre-defined promotion gates and rollback criteria.
EOF

  cat >"$fixtures_dir/constitution-fail-8-1-semantics.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
Security, capability, and platform constraints are non-bypassable and must be applied before lower-priority policy layers.
## 5.3 Determinism
Given identical explicit inputs, policy outcomes must be deterministic.
## 7.3 Acceptance Criteria
Spec changes must define measurable acceptance gates and pass them before broad rollout.
## 8.1 Testing Baseline
This text omits boundary-focused TDD requirements.
## 8.6 Privacy Baseline
No raw user message content may be logged in policy telemetry.
## 9. Rollout And Gates
Rollout requires pre-defined promotion gates and rollback criteria.
EOF

  cat >"$fixtures_dir/constitution-fail-8-6-semantics.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
Security, capability, and platform constraints are non-bypassable and must be applied before lower-priority policy layers.
## 5.3 Determinism
Given identical explicit inputs, policy outcomes must be deterministic.
## 7.3 Acceptance Criteria
Spec changes must define measurable acceptance gates and pass them before broad rollout.
## 8.1 Testing Baseline
Behavioral changes require TDD coverage for both pass and fail boundaries.
## 8.6 Privacy Baseline
This text omits privacy telemetry constraints.
## 9. Rollout And Gates
Rollout requires pre-defined promotion gates and rollback criteria.
EOF

  cat >"$fixtures_dir/constitution-fail-9-semantics.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
Security, capability, and platform constraints are non-bypassable and must be applied before lower-priority policy layers.
## 5.3 Determinism
Given identical explicit inputs, policy outcomes must be deterministic.
## 7.3 Acceptance Criteria
Spec changes must define measurable acceptance gates and pass them before broad rollout.
## 8.1 Testing Baseline
Behavioral changes require TDD coverage for both pass and fail boundaries.
## 8.6 Privacy Baseline
No raw user message content may be logged in policy telemetry.
## 9. Rollout And Gates
This text omits promotion and rollback gate language.
EOF

  cat >"$fixtures_dir/constitution-fail-invalid-section-9-heading.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
## 5.3 Determinism
## 7.3 Acceptance Criteria
## 8.1 Testing Baseline
## 8.6 Privacy Baseline
## 9 Success criteria
EOF

  cat >"$fixtures_dir/constitution-fail-wrong-heading-level.md" <<'EOF'
# Constitution Fixture

### 5.2 Policy Precedence
## 5.3 Determinism
## 7.3 Acceptance Criteria
## 8.1 Testing Baseline
## 8.6 Privacy Baseline
## 9. Rollout And Gates
EOF

  cat >"$fixtures_dir/constitution-fail-inline-false-positive.md" <<'EOF'
# Constitution Fixture

## 5.2 Policy Precedence
## 5.3 Determinism
## 7.3 Acceptance Criteria
## 8.1 Testing Baseline
Reference text only: see ## 8.6 section traceability guidance.
## 9. Rollout And Gates
EOF
}
