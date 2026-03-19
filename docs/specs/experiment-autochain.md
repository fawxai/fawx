# Spec: Experiment Auto-Chain (Iterative Refinement)

## Problem

Each `fawx experiment run` is a single shot. When an experiment scores 1.00 but gets REJECTED due to 1 failing test out of 804, the user must manually re-run. The chain-forward feature already feeds prior results into the next attempt, but there's no automated loop.

## Solution

Add `--max-rounds N` to `fawx experiment run`. The runner loops: run → evaluate → if REJECT and promising, re-run with chain history → repeat until ACCEPT or max rounds.

## CLI Changes

```
fawx experiment run \
  --mode subagent \
  --signal "missing tests" \
  --hypothesis "add tests to scoring" \
  --project ~/fawx \
  --scope "engine/crates/fx-consensus/src/scoring.rs" \
  --nodes 1 \
  --timeout 600 \
  --max-rounds 5        # NEW: default 1 (current behavior)
```

## Runner Changes

File: `engine/crates/fx-consensus/src/runner.rs`

Current flow:
```
run_experiment() → single experiment → record chain entry → return
```

New flow:
```
run_experiment_loop(max_rounds) →
  for round in 1..=max_rounds:
    run single experiment (chain history auto-injected by chain-forward)
    record chain entry
    if decision == Accept → return early (success)
    if decision == Reject:
      if score == 0.0 → return early (no progress, don't waste rounds)
      else → continue (promising, try again)
    if decision == Inconclusive → return early (no candidates produced)
  return final result
```

### Stop conditions
- **ACCEPT**: Stop. The patch passed all gates. Winner found.
- **REJECT with score 0.0**: Stop. Build failed or no tests passed. Subagent produced nothing useful. Burning more rounds won't help without a different signal/scope.
- **REJECT with score > 0.0**: Continue. The patch was close (e.g., 803/804 tests passed). Chain-forward will tell the next subagent what went wrong.
- **INCONCLUSIVE**: Stop. No candidates were produced.
- **Max rounds reached**: Stop. Return the best result from all rounds.

### Why score == 0.0 is the cutoff
A score of 0.0 means the build failed or all tests failed. The subagent either couldn't generate valid code or the approach is fundamentally wrong. Retrying with "your build failed" isn't enough — the subagent needs a different signal or scope. Scores > 0.0 mean partial success — the code compiled and some/all tests passed. These are refinable.

## Chain-Forward Integration

Already implemented (#1364). Each round automatically:
1. Reads chain entries for the same signal
2. Formats prior results: score, decision, patch preview, test results
3. Injects into the subagent prompt: "Previous attempt scored X, decision Y, issue was Z"

The subagent sees cumulative history across rounds. Round 3's prompt includes rounds 1 and 2.

## CLI Output

```
═══ Experiment Round 1/5 ═══
...
Decision: ❌ REJECT (score: 1.00, 803/804 tests passed)
Continuing — promising result, retrying with chain history...

═══ Experiment Round 2/5 ═══
...
Decision: ✅ ACCEPT (score: 1.00, 804/804 tests passed)

═══ Auto-chain complete: ACCEPT after 2 rounds ═══
```

## Scope

### In scope
- `--max-rounds N` CLI flag (default 1)
- Loop logic in runner
- Round display in CLI output
- Stop conditions as specified above

### Out of scope (future)
- Automatic signal detection (user still provides signal + hypothesis)
- Cross-experiment learning (chain-forward handles same-signal only)
- Parallel rounds (sequential only for now)
- `--auto` mode that loops indefinitely until ACCEPT (dangerous, needs budget controls)

## Files Changed

| File | Change |
|------|--------|
| `engine/crates/fx-consensus/src/runner.rs` | Add `run_experiment_loop`, loop logic, stop conditions |
| `engine/crates/fx-cli/src/commands/experiment/mod.rs` | Add `--max-rounds` arg, pass to runner |
| `engine/crates/fx-tools/src/experiment_tool.rs` | Pass max_rounds from tool params |

## Testing

1. Unit test: loop stops on ACCEPT after round 1
2. Unit test: loop stops on score 0.0 (no progress)
3. Unit test: loop continues on REJECT with score > 0.0
4. Unit test: loop stops at max_rounds
5. Unit test: default max_rounds=1 preserves current behavior
