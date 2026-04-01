# Step 13 — PR-Sized Execution Pack

## Purpose

This folder breaks Step 13 into PR-sized implementation specs that a local Codex agent can execute one at a time without having to infer the decomposition plan from the umbrella spec.

Use these only after:
- Step 12 / `#1642` is merged on `dev`
- detached-lane stabilization from `#1673` is already trusted
- the working branch starts from fresh `origin/dev`

## Execution rules

- One file in this folder = one PR-sized slice
- Run slices sequentially, not in parallel
- Fresh worktree for every slice
- Branch from current `origin/dev` every time
- No stacked PR tower unless a slice proves impossible to separate cleanly
- Each slice is a pure structural refactor unless the spec explicitly says otherwise

## Global validation gate

Every slice must pass:

```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

For slices that touch behavior-heavy loop paths, also preserve targeted regression coverage for:
- direct inspection
- direct utility
- bounded local
- continuation behavior
- tool ordering / replay integrity
- compaction behavior where applicable

## Suggested execution order

1. `step13-1-streaming-bridge.md`
2. `step13-2-retry-policy.md`
3. `step13-3-request-builders.md`
4. `step13-4a-compaction-helpers.md`
5. `step13-4b-compaction-entrypoints.md`
6. `step13-5a-decomposition-planning.md`
7. `step13-5b-decomposition-execution.md`
8. `step13-6a-tool-execution-core.md`
9. `step13-6b-tool-loop-policy.md`
10. `step13-6c-tool-synthesis-fallback.md`
11. `step13-7-test-reorg-thin-orchestrator.md`

## Fresh worktree template

```bash
git fetch origin
git worktree add /tmp/fawx-step13-<slice> origin/dev
cd /tmp/fawx-step13-<slice>
git checkout -b codex/step13-<slice-name>
git reset --hard origin/dev
```

## Notes for implementers

- Do not redesign the architecture while implementing a slice. The slice spec is the contract.
- Do not quietly expand scope because related code is nearby.
- If a clean boundary is impossible, stop and write down the blocker instead of improvising a larger refactor.
- No name-matching dispatch tables. No static registries. No plugin framework detours.
- The target end state is a thin `loop_engine/mod.rs` orchestrator, but each slice should stand on its own.
