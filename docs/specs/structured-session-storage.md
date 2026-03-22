# Structured Session Storage (PR C)

## Problem

`SessionMessage.content` is a plain `String`. This means:

1. **Tool calls are lost on reload.** When a session is persisted and reloaded, only the text portion survives. Tool use blocks, tool results, and images are stripped. The model loses tool call context after a restart.

2. **No per-message token accounting.** There's no way to know how many tokens each message consumed, making budget tracking imprecise and context window management guesswork.

3. **Export is lossy.** `fawx sessions export` outputs flat text, making it useless for replaying conversations or analyzing tool use patterns.

## Solution

### 1. Structured `SessionMessage.content`

Replace `content: String` with `content: Vec<SessionContentBlock>` in `fx-session`.

**New type in `fx-session/src/session.rs`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: serde_json::Value },
    Image { media_type: String },  // data NOT stored — just marker
}
```

**Updated `SessionMessage`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: Vec<SessionContentBlock>,
    pub timestamp: u64,
    pub token_count: Option<u32>,
}
```

### 2. Migration (backward compatible)

The store uses `serde_json` serialization. Old sessions have `content: "string"`. New sessions have `content: [{"type": "text", "text": "..."}]`.

Add a custom deserializer on `SessionMessage.content`:

```rust
fn deserialize_content<'de, D>(deserializer: D) -> Result<Vec<SessionContentBlock>, D::Error>
where D: serde::Deserializer<'de>
{
    // Try Vec<SessionContentBlock> first
    // Fall back to String → vec![SessionContentBlock::Text { text: s }]
}
```

This means:
- Old sessions load without migration (string → single Text block)
- New sessions persist structured blocks
- Re-saving an old session upgrades it automatically

### 3. Conversion bridge

Add `From<fx_llm::ContentBlock> for SessionContentBlock` and the reverse in `fx-session`:

```rust
impl From<ContentBlock> for SessionContentBlock {
    fn from(block: ContentBlock) -> Self {
        match block {
            ContentBlock::Text { text } => Self::Text { text },
            ContentBlock::ToolUse { id, name, input } => Self::ToolUse { id, name, input },
            ContentBlock::ToolResult { tool_use_id, content } => Self::ToolResult { tool_use_id, content },
            ContentBlock::Image { media_type, .. } => Self::Image { media_type },
        }
    }
}
```

Note: `Image` drops the base64 `data` field to avoid bloating the session store. Only the marker is preserved so the model knows an image was sent.

### 4. Per-message token accounting

`token_count: Option<u32>` on `SessionMessage`. Populated from the LLM response's usage data when available. `None` for messages where token count is unknown (user input, legacy messages).

### 5. Wire into existing code

**`fx-cli/src/headless.rs`:**
- Where conversation history is built for the LLM, convert `SessionContentBlock` → `ContentBlock`
- After LLM response, convert response `ContentBlock`s → `SessionContentBlock`s and persist
- Populate `token_count` from `CompletionResponse.usage`

**`fx-api/src/handlers/sessions.rs`:**
- `GET /v1/sessions/:key/messages` returns structured content blocks
- Export endpoint includes tool calls

**`fx-cli/src/commands/sessions.rs`:**
- `fawx sessions export` renders tool calls in a readable format

## Files to change

| File | Change |
|------|--------|
| `engine/crates/fx-session/src/session.rs` | New `SessionContentBlock` enum, update `SessionMessage`, custom deserializer |
| `engine/crates/fx-session/src/lib.rs` | Re-export `SessionContentBlock` |
| `engine/crates/fx-session/Cargo.toml` | Add `serde_json` dep if not present |
| `engine/crates/fx-cli/src/headless.rs` | Convert between `ContentBlock` ↔ `SessionContentBlock` when building/saving history |
| `engine/crates/fx-api/src/handlers/sessions.rs` | Update response types for structured content |
| `engine/crates/fx-cli/src/commands/sessions.rs` | Update export formatting |

## Tests required

1. **Roundtrip:** Create `SessionMessage` with mixed content blocks, serialize, deserialize, assert equality
2. **Migration:** Deserialize old-format JSON (`content: "hello"`) into `Vec<SessionContentBlock::Text>`
3. **Conversion:** `ContentBlock` → `SessionContentBlock` → `ContentBlock` roundtrip (minus image data)
4. **Token accounting:** `token_count` persists and loads correctly, `None` for legacy
5. **Export:** Structured messages render tool calls in export output

## Out of scope

- Streaming token counting (per-chunk accounting) — future
- Image data storage (intentionally markers only)
- Session compaction changes (compactor already operates on `Vec<Message>`, not `SessionMessage`)
- redb schema migration tooling (serde handles it transparently)
