# Spec: Write-File Content Reference Pattern

## Problem

LLMs frequently generate malformed JSON when `write_file` content contains backticks, nested quotes, or complex multi-line strings. The model must JSON-encode content (escaping every `"` as `\"`, every newline as `\n`, every backslash as `\\`), which it botches — especially with markdown, code blocks, or heredoc syntax.

When `parse_tool_arguments_object` catches the parse failure, it wraps the raw string as `{"__fawx_raw_args": "..."}`. But `write_file` expects `{"path": "...", "content": "..."}`, so deserialization fails with "missing required field." The model then spirals through workarounds (`run_command` with heredoc, splitting content, etc.) that hit the same escaping issue.

## Solution: `content_ref` Parameter

Add an optional `content_ref` parameter to `write_file` that references content from a prior tool result, avoiding JSON encoding entirely.

### How It Works

1. Model calls `read_file` or another content-producing tool
2. The tool result (with its `tool_call_id`) contains the content
3. Model calls `write_file` with `{"path": "output.md", "content_ref": "call_abc123"}` instead of embedding the content in `"content"`
4. `write_file` handler looks up the referenced tool result from the current cycle's results and uses its output as the content

For **new** content (not from a prior read), the model can use a two-step pattern:
1. Call a new `draft_content` tool: `{"label": "my-spec", "content": "..."}`  — stores the content in a cycle-scoped draft buffer
2. Call `write_file` with `{"path": "spec.md", "content_ref": "draft:my-spec"}`

### `draft_content` Tool

A lightweight tool that stores content for later reference. The content is held in memory for the current cycle only.

```
Tool: draft_content
Parameters:
  label: string (required) — identifier for this draft
  content: string (required) — the content to store
Returns: "Draft 'my-spec' stored (1234 bytes)"
```

The key insight: `draft_content` arguments are simpler JSON because the `label` is short. If `content` itself has escaping issues, the `__fawx_raw_args` fallback can still recover — we just need a lenient parser that extracts label and content from the raw string even when JSON is malformed.

### Why This Works

- `content_ref` is a short string (a tool call ID or `draft:label`) — no escaping issues
- For read→transform→write flows, content never passes through JSON encoding at all
- For new content, `draft_content` is a simpler target for the raw-args fallback (label is always simple)
- Backward compatible: `content` parameter still works when the model manages to encode it correctly

## Implementation

### Changes

1. **`engine/crates/fx-tools/src/tools.rs`**:
   - Add `content_ref: Option<String>` to `WriteFileArgs`
   - Add `draft_content` tool definition and handler
   - In `handle_write_file`: if `content_ref` is set, resolve from cycle context; if `content` is set, use directly; if neither, error
   - Add `DraftBuffer` (HashMap<String, String>) to `BuiltinToolExecutor`

2. **`engine/crates/fx-tools/src/tools.rs`** (tool definition):
   - Add `content_ref` to `write_file` schema with description
   - Add `draft_content` tool definition

3. **`engine/crates/fx-kernel/src/loop_engine.rs`** (cycle context):
   - Pass tool results from the current cycle into tool executor so `content_ref` can look up prior results
   - Clear draft buffer at cycle start

### Draft Buffer Recovery

When `draft_content` receives `__fawx_raw_args`, attempt regex extraction:
- Find `"label"\s*:\s*"([a-zA-Z0-9_-]+)"` for the label (matches the format constraint)
- Everything between the label field and the closing `}` (or end of string) is content
- This is a best-effort fallback; the primary path is valid JSON
- **Label format constraint**: Labels must match `[a-zA-Z0-9_-]{1,64}` to keep regex extraction reliable

### Error Messages for `content_ref`

- `content_ref` referencing a nonexistent draft: `"Draft 'my-label' not found. Available drafts: [list]. Create it with draft_content first."`
- `content_ref` referencing a stale tool_call_id (from a prior cycle): `"Tool result 'call_abc' is from a previous cycle and no longer available. Re-read the content or use draft_content."`
- `content_ref` referencing a tool result that returned an error: `"Tool result 'call_abc' is an error result ('permission denied'). Cannot use error output as file content. Fix the underlying error first."` — Validation: check `result.success` / `is_error` flag before using content.

### `write_file` Tool Definition Update

```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string" },
    "content": { "type": "string", "description": "File content (inline)" },
    "content_ref": {
      "type": "string",
      "description": "Reference to content from a prior tool result (tool_call_id) or draft (draft:label). Use this instead of content when writing complex/multi-line text to avoid JSON escaping issues."
    }
  },
  "required": ["path"]
}
```

**Precedence**: If both `content` and `content_ref` are provided, `content_ref` wins. This avoids ambiguity and encourages the model to use the reference pattern.

### Cycle Context Plumbing

The tool executor needs access to:
1. Current cycle's tool results (for `content_ref` referencing a tool_call_id)
2. Draft buffer (for `content_ref` referencing `draft:label`)

Options:
- **A) Thread-local / cycle-scoped state on executor** — Add `cycle_results: Vec<ToolResult>` and `drafts: HashMap<String, String>` to `BuiltinToolExecutor`. Kernel updates these before each tool execution batch.
- **B) Context parameter on execute_tools** — Expand `execute_tools` signature with a context object. Cleaner but changes the trait.

Recommend **A** for minimal trait changes. The kernel already has access to the executor via `self.tool_executor` and can set cycle state before execution.

## Tests

1. `write_file_with_content_ref_from_draft` — draft_content + write_file with draft:label
2. `write_file_with_content_ref_missing_draft_fails` — content_ref to nonexistent draft
3. `write_file_prefers_content_ref_over_content` — both present, content_ref wins
4. `write_file_requires_content_or_content_ref` — neither present, error
5. `draft_content_stores_and_retrieves` — basic draft_content round-trip
6. `draft_content_raw_args_recovery` — malformed JSON still extracts label+content
7. `draft_buffer_clears_between_cycles` — drafts don't leak across cycles

## Migration

- No breaking changes; `content` still works
- System prompt guidance (in tool description) nudges model toward `content_ref` for complex content
- Over time, models learn to prefer the ref pattern for multi-line content
