# Spec: Fix Telegram Image Content Blocks Not Reaching LLM

## Bug
When a user sends a photo via Telegram, Claude responds "I don't have vision capabilities" — the image content blocks never reach the LLM API.

## Root Cause
In `engine/crates/fx-kernel/src/loop_engine.rs`, the perceive step builds a `context_window` that includes the user message WITH image content blocks (`build_user_message` at ~line 1307). Then `build_reasoning_request` (~line 4483) appends a SECOND user message via `build_processed_perception_message` — also with image content blocks.

This produces two consecutive user-role messages in the request. The Anthropic Messages API requires alternating user/assistant roles. The API likely auto-merges these but drops the image content blocks in the process.

## Fix
**Do NOT include images in the context_window user message.** Images should only appear in the reasoning request's final user message (via `build_processed_perception_message`).

### Changes to `engine/crates/fx-kernel/src/loop_engine.rs`:

1. **`build_user_message`** (~line 216): Remove image handling. Always use `Message::user(user_message)`, never `Message::user_with_images`. The context_window user message is for conversation history — images don't belong there (they'd be huge in history and get mangled by compaction anyway).

2. **`build_processed_perception_message`** (~line 225): Keep as-is. This correctly attaches images to the reasoning prompt.

3. The `ProcessedPerception.images` field and `build_perception_snapshot_with_images` should remain unchanged — they correctly carry images from perception to reasoning.

### Also add tracing for the image path:

4. In `engine/crates/fx-cli/src/http_serve.rs`, add an `info!` log line after successful photo encoding, e.g.:
   ```
   tracing::info!(count = images.len(), "encoded Telegram photos as content blocks");
   ```
   This goes in `encode_photos` after the loop, before returning.

5. In `engine/crates/fx-kernel/src/loop_engine.rs`, add a trace-level log in `build_processed_perception_message` when images are attached:
   ```
   tracing::trace!(image_count = images.len(), "attaching image content blocks to reasoning request");
   ```

## Tests

1. **`build_user_message_excludes_images`**: Create a PerceptionSnapshot with images in UserInput, call `build_user_message`, verify the resulting Message has only a Text content block (no Image blocks).

2. **`build_processed_perception_message_includes_images`**: Create a ProcessedPerception with images, call `build_processed_perception_message`, verify the resulting Message contains Image content blocks.

3. **`reasoning_request_has_single_user_message_with_images`**: Build a full reasoning request with images, verify:
   - No consecutive same-role messages
   - The final user message contains image content blocks
   - Earlier messages (context_window) do NOT contain image content blocks

## Constraints
- Only modify `loop_engine.rs` and `http_serve.rs`
- Do NOT change the Anthropic provider, fx-llm types, or perception types
- Run `cargo fmt --all` before committing
- Run `cargo clippy --workspace --tests -- -D warnings` before committing
- Run `cargo test --workspace` before committing
