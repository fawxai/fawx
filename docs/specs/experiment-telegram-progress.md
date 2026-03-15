# Telegram Progress for Experiments

## Problem

When experiments run via Telegram tool call, the user sees nothing until the final result. Multi-minute silence in a chat interface feels broken.

## Solution

A single Telegram message that updates in-place via `edit_message` as the experiment progresses.

## Design

### Message lifecycle

1. **Experiment starts** → send initial message: "⚗ Experiment starting..."
2. **Progress events** → edit message with accumulated progress
3. **Experiment completes** → final edit with result summary

### Message format

```
⚗ Experiment: missing tests
Round 1/5

▸ Collecting baseline...
✓ Baseline: 71 tests (2.4s)
▸ node-0 generating patch...
✓ Patch: 180 lines
▸ Evaluating...
✓ build ✓ | tests 72/72 | signal ✓ | safety ✓

Score: node-0 → 1.00
Decision: ✅ ACCEPT
```

### Rate limiting

Telegram limits `editMessageText` to ~30 calls per minute per chat. Batch events:
- Buffer events for 1 second
- Edit message with all buffered events at once
- Never edit more than once per second

### Implementation

The experiment tool in `fx-tools/src/experiment_tool.rs` needs access to a channel sender for Telegram messages. When running in a Telegram context:

1. Tool executor creates a `ProgressCallback` that:
   - Formats events using `format_progress_event`
   - Appends to an accumulated message string
   - Debounces edits (1-second minimum interval)
   - Calls `edit_message` via the channel API

2. First event triggers `send_message` (gets message_id)
3. Subsequent events trigger `edit_message` on that message_id
4. Final event sends the complete result

### Channel integration

The `ProgressCallback` needs access to the Telegram channel for `edit_message`. Options:

**Option A (simpler):** Pass a `TelegramProgressSender` into the tool that handles message lifecycle:
```rust
pub struct TelegramProgressSender {
    channel: Arc<dyn TelegramChannel>,
    chat_id: String,
    message_id: Option<i64>,
    buffer: String,
    last_edit: Instant,
}
```

**Option B (generic):** Define a `ProgressSink` trait that different channels implement. TUI, Telegram, CLI each provide their own sink. This is cleaner but more work.

Recommend Option A for now, extract to Option B if we add more channels.

## Files to modify

1. `fx-channel-telegram/src/progress.rs` — new file, TelegramProgressSender
2. `fx-tools/src/experiment_tool.rs` — wire progress sender when Telegram context available
3. `fx-channel-telegram/src/lib.rs` — export progress sender

## Scope

~150 lines new code. Single PR. Depends on #1368 (verbose/progress callbacks).
