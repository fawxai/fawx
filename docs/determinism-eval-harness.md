# Determinism Eval Harness

Tracks loop-confidence metrics for deterministic fallback behavior (issue #833) and harness automation work (issue #835).

Current harness scope is **baseline-only** with fixed synthetic scenarios for stable CI diffs.
Live agent/state-machine execution realism is a follow-up tracked under issue #835.

## Modes
- `ci-lite`: fast subset (3 scenarios, one per domain)
- `full`: broader suite (9 scenarios)

Domains covered in both modes:
- travel
- shopping
- general web research

## Command

```bash
fawx eval-determinism \
  --mode ci-lite \
  --output .ci/determinism/latest-report.json \
  --baseline .ci/determinism/baseline-ci-lite.json
```

Nightly/manual full run:

```bash
fawx eval-determinism \
  --mode full \
  --output .ci/determinism/latest-full-report.json \
  --baseline .ci/determinism/baseline-full.json
```

Update baseline snapshot:

```bash
fawx eval-determinism --mode full --update-baseline
```

## CI automation

Workflow: `.github/workflows/determinism-eval.yml`

- Pull requests: runs `ci-lite` mode and uploads JSON artifact + markdown summary.
- Nightly schedule: runs `full` mode.
- Manual trigger (`workflow_dispatch`): run `ci-lite` or `full` on demand.
- PR runs post/update a sticky comment with the latest metric summary.

## Machine-readable output
JSON report fields:
- `metrics.false_success_claim_rate`
- `metrics.completion_artifact_pass_rate`
- `metrics.deterministic_fallback_correctness`
- `metrics.retry_bound_adherence`
- `trend_vs_baseline.*` deltas when baseline is present

This output is intended for CI checks and PR-comment summarization.
