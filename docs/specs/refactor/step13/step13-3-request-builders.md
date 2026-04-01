# Step 13.3 — Request Builder Extraction

## Branch
`codex/step13-3-request-builders`

## Goal
Extract reasoning request construction and prompt assembly into `loop_engine/request.rs`.

## Why this slice exists
These functions are mostly pure and are some of the least-coupled logic remaining in `loop_engine/mod.rs`. They should move before compaction and tool execution work.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/request.rs`
- related tests

## Move
- reasoning request builders
- reasoning message builders
- system prompt builders
- user prompt builders
- processed-perception message construction helpers
- related pure formatting/build helpers used only by request construction

## Keep in `mod.rs`
- the actual `reason()` orchestration path
- provider invocation
- streaming bridge usage
- response handling

## Non-goals
- No prompt redesign
- No provider-specific behavior changes
- No changes to message ordering or content semantics
- No merging request-building with streaming or tool execution

## Acceptance criteria
- `request.rs` is the single owner of LLM request construction helpers
- `reason()` calls these helpers through imports only
- request payload semantics remain identical
- tests verify unchanged request/message construction behavior

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

If there are request-shape tests, keep them green or move them with the extracted code.

## Reviewer focus
- Did the slice stay purely about request construction?
- Are the extracted functions still explicit and typed?
- Was there any accidental prompt-content drift?
