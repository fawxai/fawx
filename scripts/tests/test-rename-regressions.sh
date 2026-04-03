#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

WORKFLOW_FILE="$ROOT_DIR/.github/workflows/determinism-eval.yml"
VALIDATOR_FILE="$ROOT_DIR/scripts/validate-h24-spec-contract.sh"

grep -Fq 'cargo run -p fx-cli --bin fawx -- eval-determinism \' "$WORKFLOW_FILE"
if grep -Fq 'cargo run -p ct-cli --bin fawx -- eval-determinism \' "$WORKFLOW_FILE"; then
  echo "FAIL: determinism workflow still references ct-cli" >&2
  exit 1
fi

grep -Fq 'Fawx needs both session-type and model-tier behavior.' "$VALIDATOR_FILE"
grep -Fq 'Fawx is fixed-tool mobile architecture.' "$VALIDATOR_FILE"
grep -Fq 'Fawx must prioritize mobile latency and token cost more aggressively on `SMALL` tier.' "$VALIDATOR_FILE"
grep -Fq '### 3.2 What to Adapt (Fawx-Specific)' "$VALIDATOR_FILE"

if grep -Fq 'Citros needs both session-type and model-tier behavior.' "$VALIDATOR_FILE"; then
  echo "FAIL: validator still contains Citros-specific section text" >&2
  exit 1
fi

if grep -Fq '### 3.2 What to Adapt (Citros-Specific)' "$VALIDATOR_FILE"; then
  echo "FAIL: validator still expects Citros-Specific heading" >&2
  exit 1
fi

echo "rename regression checks passed"
