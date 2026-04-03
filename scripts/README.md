# Scripts

This directory contains the public-promotion guard and the H2.4 spec contract validator.

## Public Promotion Guard

Use this from a promotion branch that is based on `public/main` after cherry-picking only OSS-safe commits.

### Run Locally

Python 3.11+ is required for the guard and its regression tests because `check_public_promotion.py` uses `tomllib`.

- Fetch the public base ref if needed:
  - `git fetch public main`
- Run the guard:
  - `scripts/check-public-promotion`
- Run the guard regression tests:
  - `python3 -m unittest scripts/tests/test_check_public_promotion.py`

The guard compares the current branch against `public/main`, fails on blocked or non-allowlisted paths, scans added lines for private markers, and checks a few public invariants before you open a public PR.

## H2.4 Spec Contract Validator

### Run Locally

- Validate the current spec contract:
  - `scripts/validate-h24-spec-contract.sh`
- Validate and emit JSON artifact:
  - `VALIDATOR_JSON_OUT=/tmp/h24-spec-contract-summary.json scripts/validate-h24-spec-contract.sh`
- Run validator tests:
  - `scripts/tests/test-validate-h24-spec-contract.sh`
- Force fallback backend coverage (no `rg` path) deterministically:
  - `FORCE_NO_RG=1 scripts/tests/test-validate-h24-spec-contract.sh`
- Run rename regression checks for CI workflow + validator literals:
  - `scripts/tests/test-rename-regressions.sh`

Both validators can be run from repo root.

## What This Validator Intentionally Enforces

The validator is line-oriented and strict by design. It enforces normative contract text in:

- OpenClaw copy/adapt/do-not-copy sections.
- Invariants and invariant-to-test mappings.
- Mode x tier matrix rows and policy rules.
- Prompt budget rows, trim-order rules, and non-trimmable lists.
- Canonical safety clauses and shortening rules.
- Runtime schema and runtime redaction bullets.
- Rollback trigger thresholds and key section headings.
- Machine-readable outputs:
  - `validator_status=pass`
  - `validator_search_backend=<rg|grep>`
  - `verified_clause_count=<int>`
  - `validator_summary_json=<compact JSON>`
  - Optional JSON artifact file when `VALIDATOR_JSON_OUT` is set.

`FORCE_NO_RG=1` is preferred over PATH-only simulation to guarantee fallback coverage on merged-usr systems where `rg` may still be discoverable.

## Edits Expected to Break Contract Checks

These changes are expected to fail validation unless the validator/tests are updated in the same PR:

- Rewording/removing required normative bullets or headings.
- Changing numeric thresholds, matrix rows, or budget values.
- Altering trim-order or non-trimmable rule lines.
- Modifying canonical safety or runtime-redaction contract lines.
- Moving/removing required test IDs from required sections.

When contract semantics need to change, update all three together:

1. `docs/specs/h2-4-model-aware-prompt-tuning-spec.md`
2. `scripts/validate-h24-spec-contract.sh`
3. `scripts/tests/test-validate-h24-spec-contract.sh`
