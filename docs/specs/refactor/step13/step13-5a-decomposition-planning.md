# Step 13.5a — Decomposition Planning and Aggregation

## Branch
`codex/step13-5a-decomposition-planning`

## Goal
Start the decomposition extraction by moving planning, allocation, and aggregation logic into `loop_engine/decomposition.rs` without yet moving the full recursive execution paths.

## Why this slice exists
The decomposition subsystem is too important and too recursive to move in one shot. The planning side is the safest first cut.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/decomposition.rs`
- related tests

## Move
- decomposition planning helpers
- budget allocation helpers
- aggregation helpers
- high-level decomposition result shaping where separable

## Keep in `mod.rs` for now
- recursive sub-goal execution paths
- sequential/concurrent sub-goal execution loops
- child engine construction and follow-up execution

## Non-goals
- No concurrency strategy changes
- No child-engine behavior changes
- No depth-limit changes
- No follow-up decomposition behavior changes

## Acceptance criteria
- planning/allocation/aggregation logic lives in `decomposition.rs`
- recursive execution remains untouched in this slice
- decomposition tests for planning and aggregation move with the code or are added alongside it
- behavior remains unchanged

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Retain targeted decomposition regressions if they already exist.

## Live clean-bisect API smoke test
Use `docs/runbooks/clean-bisect-lane.md` on a fresh detached lane. Use the headless API only.

### Test cases
1. **Multi-part read request**
   - Prompt: `Read README.md and Cargo.toml, then tell me: (1) the first heading in README.md, and (2) the workspace or package name in Cargo.toml.`
   - Pass if both answers are correct and grounded in the detached files.
2. **Three-part aggregation check**
   - Prompt: `Read README.md and docs/specs/refactor/1642-headless-decomposition.md, then list one concrete goal from each file.`
   - Pass if the response returns one valid item from each file without dropping part of the request.
3. **Clean lane check**
   - Prompt: `Run git status and confirm whether anything changed.`
   - Pass if the detached lane remains clean.

## Done means
- decomposition planning/allocation/aggregation logic is isolated cleanly
- workspace validation passes
- the live clean-bisect API smoke test above passes on a fresh lane after implementation

## Reviewer focus
- Did this slice stop before the recursive execution boundary?
- Are planning and aggregation now clearly owned by the decomposition subsystem?
- Was any execution behavior changed accidentally while extracting the helpers?
