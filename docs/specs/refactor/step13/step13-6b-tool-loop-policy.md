# Step 13.6b — Tool Loop Policy Integration

## Branch
`codex/step13-6b-tool-loop-policy`

## Goal
Move the higher-level tool-loop policy into `loop_engine/tool_execution.rs` after the core execution machinery is already extracted.

## Why this slice exists
This is the highest-risk behavioral slice in Step 13. It contains the logic that ties tool execution to retry policy, continuation policy, nudging, strip-tools thresholds, and budget enforcement.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/tool_execution.rs`
- related tests

## Move
- retry integration for tool loops
- nudge / strip-tools integration
- continuation compaction glue
- multi-round tool-loop policy helpers
- other policy code that conceptually belongs to tool execution rather than the top-level orchestrator

## Preconditions
This slice should not start until:
- 13.2 retry policy is merged
- 13.4a / 13.4b compaction extraction is merged
- 13.6a tool execution core is merged

## Non-goals
- No synthesis fallback move yet
- No changes to direct inspection / direct utility routing
- No new policy heuristics
- No hidden behavioral rewrite under the label of extraction

## Acceptance criteria
- tool-loop policy lives with tool execution rather than spread across `mod.rs`
- retry / nudge / continuation semantics remain unchanged
- existing regressions around runaway continuation, tool ordering, and grouped history remain green
- the interface stays explicit and typed

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

This slice should preserve regressions for:
- mixed-text runaway fixes
- tool ordering / replay integrity
- turn-scoped grouped tool history
- multi-round continuation behavior

## Live clean-bisect API smoke test
Use `docs/runbooks/clean-bisect-lane.md` on a fresh detached lane. Use the headless API only.

### Test cases
1. **Failure then recovery with write**
   - Prompt: `Read DOES_NOT_EXIST_STEP13.md. If it is missing, read README.md instead, append this exact marker on a new last line: <!-- STEP13_TOOL_POLICY_MARKER -->, then tell me exactly what you appended.`
   - Pass if the assistant recovers from the failed read, performs the write, and terminates cleanly without looping.
2. **Follow-up status check**
   - Prompt: `Run git status and tell me which file is modified.`
   - Pass if the response reports only detached `README.md` as modified.
3. **No runaway / ordering regression**
   - Prompt: `Quote the exact line you appended and stop.`
   - Pass if the assistant returns the exact marker and does not enter a repeated tool loop or surface tool-order errors.

## Done means
- tool-loop policy is isolated cleanly
- workspace validation passes
- the live clean-bisect API smoke test above passes on a fresh lane after implementation

## Reviewer focus
- Did the PR preserve current loop-policy semantics?
- Is this still a module extraction, not a stealth rewrite?
- Are retry/compaction/continuation boundaries clearer after the move?
