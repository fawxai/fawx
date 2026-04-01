# Step 13.7 — Test Reorg and Thin Orchestrator Cleanup

## Branch
`codex/step13-7-test-reorg-thin-orchestrator`

## Goal
Finish Step 13 by reorganizing loop-engine tests by subsystem and narrowing `loop_engine/mod.rs` to a thin orchestrator.

## Why this slice exists
Earlier slices move the real subsystem code. This final slice finishes the structural cleanup: `mod.rs` should read as the composition root, not as a giant mixed implementation file with a 13k-line test pile attached.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/tests/`
- any small adjacent test-module wiring needed for loop-engine submodules

## Move
- split giant loop-engine tests into subsystem-specific files
- keep only orchestrator-specific tests near the orchestrator
- finish reducing `mod.rs` to the orchestrator + builder + small top-level helpers

## Target end state
`mod.rs` should primarily contain:
- `LoopEngine` struct definition
- builder
- `run_cycle` / `run_cycle_streaming` / top-level orchestrator flow
- `reason`
- `decide`
- small top-level re-exports/helpers that genuinely belong there

## Non-goals
- No new behavior
- No test rewrites beyond what is needed to move them cleanly
- No opportunistic cleanup sweep outside loop-engine structure

## Acceptance criteria
- tests are organized by subsystem, not piled into one monolith
- `mod.rs` is materially thinner and easier to read as a composition root
- all previous subsystem extractions remain intact and readable
- final behavioral validation remains green

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Post-completion gate for the full Step 13 result:
- preserve existing profile regressions
- preserve continuation / ordering / grouped-history regressions
- pass the clean-bisect lane battery T1–T7 on the final integrated result

## Live clean-bisect API smoke test
Use `docs/runbooks/clean-bisect-lane.md` on a fresh detached lane. Use the headless API only.

### Test cases
Run the full detached-lane battery plus one final continuity check:
1. `Read README.md and quote the first paragraph exactly.`
2. `Read Cargo.toml and tell me the workspace or package name you see.`
3. `Read README.md, then append this exact marker on a new last line: <!-- STEP13_FINAL_MARKER -->`
4. `Run git status and tell me which file is modified.`
5. Fresh second lane: `Read README.md and summarize the first paragraph.`
6. `Read README.md, then tell me the exact heading that appears immediately after the first paragraph.`
7. `Read README.md, append <!-- STEP13_FINAL_EVIDENCE_MARKER --> on a new last line, then tell me exactly what line you appended.`
8. Final continuity check: `Quote both appended marker lines exactly and tell me which file is modified.`

### Pass criteria
- detached reads hit the detached lane, not the main checkout
- writes affect only detached `README.md`
- second lane stays clean and independent
- follow-up reasoning stays grounded in actual tool evidence
- final continuity check returns both exact marker lines and the correct modified file

## Done means
- tests are reorganized by subsystem
- `loop_engine/mod.rs` is a thin orchestrator
- workspace validation passes
- the live clean-bisect API smoke test above passes on a fresh lane after implementation

## Reviewer focus
- Is `mod.rs` now genuinely a thin orchestrator?
- Are tests organized around subsystem ownership?
- Did the PR avoid sneaking behavioral changes into what should be a structural finish pass?
