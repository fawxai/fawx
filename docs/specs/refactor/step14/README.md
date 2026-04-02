# Step 14: PR-Sized Execution Pack

## Purpose
This folder breaks Step 14 into PR-sized implementation specs so a local Codex agent can execute the backend archive and export work one slice at a time without having to infer the decomposition plan.

Use these only after:
- Step 13 is merged on `dev`
- the working branch starts from fresh `origin/dev`
- the implementation stays on the backend side of the contract

## Execution rules
- One file in this folder = one PR-sized slice
- Run slices sequentially, not in parallel
- Fresh worktree for every slice
- Branch from current `origin/dev` every time
- No stacked PR tower unless a slice proves impossible to separate cleanly
- No Swift UI work in this pack
- Keep delete, clear, and archive semantics distinct

## Global contract for Step 14
- archive is reversible metadata
- archive does not delete or clear message history
- default list excludes archived sessions
- explicit filters can include or isolate archived sessions
- archived sessions remain exportable
- session storage stays unified; no second archive store

## Global validation gate
Every slice must pass:

```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Every slice should keep API behavior stable for untouched endpoints.

The final slice must also pass a live headless API verification run. Use the headless API only. No TUI smoke test is required for Step 14.

## Suggested execution order
1. `step14-1-session-archive-metadata.md`
2. `step14-2-session-registry-archive-ops.md`
3. `step14-3-api-routes-and-list-filters.md`
4. `step14-4-export-backend.md`
5. `step14-5-contract-hardening-and-integration.md`

## Fresh worktree template
```bash
git fetch origin
git worktree add /tmp/fawx-step14-<slice> origin/dev
cd /tmp/fawx-step14-<slice>
git checkout -b codex/step14-<slice-name>
git reset --hard origin/dev
```

## Notes for implementers
- Do not improvise UI work into this backend step.
- Do not invent a second storage layer for archived sessions.
- If a route or payload shape must change from this plan, write down the blocker and why.
- Prefer additive metadata and explicit filters over hidden behavior.
- Step 15 will consume this contract later, so route semantics and response shapes should be clear and durable.
