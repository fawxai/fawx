# Step 13.6a — Tool Execution Core

## Branch
`codex/step13-6a-tool-execution-core`

## Goal
Begin the tool-execution extraction by moving the core tool-call execution machinery into `loop_engine/tool_execution.rs` without yet moving the heaviest policy integration.

## Why this slice exists
Tool execution is the highest-coupling area in Step 13. It needs to be split across multiple PRs. This first slice should extract the core round machinery only.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/tool_execution.rs`
- related tests

## Move
- core `act` / tool-call execution entrypoints where practical
- basic tool-round execution helpers
- core blocked-call shaping if tightly bound to base execution
- low-level helpers used only by core tool execution

## Keep for later slices
- retry/nudge/strip integration
- continuation compaction glue
- synthesis fallback
- terminal tool wrap-up helpers if they expand scope too much

## Non-goals
- No change to multi-round tool policy
- No retry redesign
- No synthesis-path changes
- No compaction policy changes

## Acceptance criteria
- a coherent tool execution core exists in `tool_execution.rs`
- existing tool-round behavior remains stable
- the PR does not drag the entire loop policy surface into the first extraction
- tests move with the extracted code or are added alongside it

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Pay attention to tool-round regressions, blocked-call behavior, and call/result ordering.

## Live clean-bisect API smoke test
Use `docs/runbooks/clean-bisect-lane.md` on a fresh detached lane. Use the headless API only.

### Test cases
1. **Core read/write tool turn**
   - Prompt: `Read README.md, append this exact marker on a new last line: <!-- STEP13_TOOL_CORE_MARKER -->, then tell me exactly what you appended.`
   - Pass if the assistant performs the read/write correctly and reports the exact appended line.
2. **Tool-state follow-up**
   - Prompt: `Run git status and tell me which file is modified.`
   - Pass if the response reports only detached `README.md` as modified.
3. **Second-turn exactness**
   - Prompt: `Quote the exact line you appended in the previous turn.`
   - Pass if the assistant quotes the exact marker with no history corruption.

## Done means
- tool execution core is isolated in its module
- workspace validation passes
- the live clean-bisect API smoke test above passes on a fresh lane after implementation

## Reviewer focus
- Did this slice stop at the core execution boundary?
- Is the new module actually cohesive, or is it just a partial dump of unrelated helpers?
- Was any policy behavior changed while moving the core?
