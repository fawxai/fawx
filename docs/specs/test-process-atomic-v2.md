# Atomic Test Process v2 (Reliability Gates)

## Goal
Replace monolithic, long-running Android test gates with small deterministic buckets that fail fast, isolate blast radius, and prevent 45-minute silent stalls.

## Principles
- **Atomic buckets:** one concern per bucket, independently runnable.
- **Fail fast for merge blockers:** only P0 blocks PRs.
- **Broader confidence off critical path:** P1 runs observationally in PR CI; P2 runs in nightly/workflow-dispatch lanes.
- **Deterministic defaults:** skip `@Flaky` in P0/P1 (`-PfawxRunFlakyTests=false`).
- **Escalate with explicit ownership:** every failing bucket has an owner and SLA.

## Gate Model

### P0 — Blocking smoke gate (target <= 10 minutes total)
Used on PRs to `staging`/`main`. Must be deterministic and fast.

| Bucket ID | Concern (single responsibility) | Command | Owner | Pass/Fail Criteria | Target Runtime |
|---|---|---|---|---|---|
| `p0.chat-lint` | Static quality of chat Android module | `./scripts/test-atomic.sh p0.chat-lint` | Android App Maintainer (`@abbudjoe`) | **Pass:** `:chat:lint` exits 0. **Fail:** any lint violation. | 2-3 min |
| `p0.core-sensor-ci` | Core sensor timeout/concurrency contract | `./scripts/test-atomic.sh p0.core-sensor-ci` | Core Runtime Maintainer (`@abbudjoe`) | **Pass:** `:core:phoneAgentApiSensorCiTest` exits 0. **Fail:** any test failure/error. | 2-3 min |
| `p0.chat-sensor-ci` | Chat sensor provider contract | `./scripts/test-atomic.sh p0.chat-sensor-ci` | Android App Maintainer (`@abbudjoe`) | **Pass:** `:chat:androidSensorProviderCiTest` exits 0. **Fail:** any test failure/error. | 1-2 min |

**P0 Merge Rule:** all P0 buckets must pass in the same CI run.

### P1 — Broader regression gate (non-blocking/nightly)
Used for wider regression detection without blocking developer velocity.

| Bucket ID | Concern | Command | Owner | Pass/Fail Criteria |
|---|---|---|---|---|
| `p1.core-unit` | Full core unit/regression suite (stable subset) | `./scripts/test-atomic.sh p1.core-unit` | Core Runtime Maintainer (`@abbudjoe`) | **Pass:** `:core:testDebugUnitTest` exits 0 with flaky tests disabled. |
| `p1.chat-unit` | Full chat unit/regression suite (stable subset) | `./scripts/test-atomic.sh p1.chat-unit` | Android App Maintainer (`@abbudjoe`) | **Pass:** `:chat:testDebugUnitTest` exits 0 with flaky tests disabled. |

**P1 Rule:** failures open/fix issues but do not block merge.

### P2 — Soak/flaky-detection lane (non-blocking/nightly)
Used to detect intermittent behavior and regression drift.

| Bucket ID | Concern | Command | Owner | Pass/Fail Criteria |
|---|---|---|---|---|
| `p2.soak-sensor-repeat` | Repeated sensor CI buckets for intermittent failures | `./scripts/test-atomic.sh p2.soak-sensor-repeat --iterations 5` | Reliability Owner (`@abbudjoe`) | **Pass:** all iterations pass. **Fail:** any iteration failure; collect first failing iteration index. |
| `p2.flaky-audit` | Execute suites with flaky tests enabled to detect newly unstable paths | `./scripts/test-atomic.sh p2.flaky-audit` | Reliability Owner (`@abbudjoe`) | **Pass:** both module suites pass. **Fail:** any failure; classify as known/new flaky. |

## Retry, Quarantine, and Flaky Policy

### Retry policy
- **P0:** no blind retries in merge gate. A failing bucket is treated as real until fixed or quarantined by explicit change.
- **P1/P2:** one controlled re-run is allowed for triage. If pass-on-retry, label as flaky suspect and track issue.

### Quarantine policy
- Tests proven flaky are annotated with `@Flaky(issue = "#...")` and tracked with an issue.
- Quarantined tests are excluded from P0/P1 by running with `-PfawxRunFlakyTests=false`.
- Quarantined coverage remains visible via P2 (`p2.flaky-audit`).

### Escalation policy on failure
1. **Bucket fails** → owner triages within 1 business day.
2. **If deterministic regression:** fix in next PR; if P0, block merge until green.
3. **If flaky/intermittent:**
   - open/update tracking issue,
   - add `@Flaky(issue = "#...")` if justified,
   - ensure scenario is covered in P2 lane.
4. **If bucket fails 3 consecutive nightly runs:** escalate to reliability incident, prioritize in active sprint, and require owner status update in PR/issue.

## Commands (local + CI)
All commands run from repo root:

```bash
# P0 (blocking smoke)
./scripts/test-atomic.sh p0.chat-lint
./scripts/test-atomic.sh p0.core-sensor-ci
./scripts/test-atomic.sh p0.chat-sensor-ci
./scripts/test-atomic.sh p0

# P1 (non-blocking broader regression)
./scripts/test-atomic.sh p1.core-unit
./scripts/test-atomic.sh p1.chat-unit
./scripts/test-atomic.sh p1

# P2 (non-blocking soak/flaky detection)
./scripts/test-atomic.sh p2.soak-sensor-repeat --iterations 5
./scripts/test-atomic.sh p2.flaky-audit
./scripts/test-atomic.sh p2
```

## CI Mapping
- **Blocking in PR CI:** `p0.chat-lint`, `p0.core-sensor-ci`, and `p0.chat-sensor-ci` run as separate required steps.
- **Non-blocking in PR CI:** `p1.core-unit` and `p1.chat-unit` run as observational steps (`continue-on-error: true`).
- **PR CI scope:** P2 soak/flaky buckets are intentionally omitted from PR runs to keep review latency bounded.
- **Nightly:** P1 and P2 execute in separate jobs so either lane can fail visibly without suppressing the other.
- **Schedule behavior note:** GitHub `schedule` events only run on the repository default branch. Use `workflow_dispatch` to validate nightly flow on non-default branches (for example, `staging`).
