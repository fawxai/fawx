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

## Live clean-bisect API smoke test
Use `docs/runbooks/clean-bisect-lane.md` on a fresh detached lane. Use the headless API only.

### Test cases
1. **Direct inspection request path**
   - Prompt: `Read README.md and quote the first paragraph exactly.`
   - Pass if the assistant returns the real paragraph with no refusal or prompt drift.
2. **Standard multi-step request path**
   - Prompt: `Read README.md, append this exact marker on a new last line: <!-- STEP13_REQUEST_MARKER -->, then tell me exactly what you appended.`
   - Pass if the assistant performs the read and append correctly and reports the exact line appended.
3. **Follow-up correctness**
   - Prompt: `Run git status and tell me which file is modified.`
   - Pass if the assistant reports only detached `README.md` as modified.

## Done means
- request-building helpers are isolated in their module
- workspace validation passes
- the live clean-bisect API smoke test above passes on a fresh lane after implementation

## Reviewer focus
- Did the slice stay purely about request construction?
- Are the extracted functions still explicit and typed?
- Was there any accidental prompt-content drift?
