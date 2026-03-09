# Phase 2a: `edit_file` Tool + `read_file` Enhancements

## Summary

Add an `edit_file` tool for surgical text replacement (match exact string → replace) and enhance `read_file` with `offset`/`limit` parameters for partial file reads. These are the two most-requested missing tools for agentic coding workflows — every major coding agent (OpenClaw, Cursor, Aider) has them.

## Motivation

- **`write_file` rewrites the entire file** — a 3-line fix in a 500-line file means sending 500 lines over the wire. Wastes tokens, risks accidental truncation, breaks concurrent edits.
- **`read_file` returns the entire file** — for large files (1000+ lines), most of the content is irrelevant. Agents waste context window on boilerplate.
- Both limitations compound in multi-tool agentic loops where the agent needs many small edits across files.

## Design

### `edit_file` Tool

```json
{
  "name": "edit_file",
  "description": "Replace exact text in a file. The old_text must match exactly (including whitespace and newlines). Use for precise, surgical edits.",
  "parameters": {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "File path (supports ~)" },
      "old_text": { "type": "string", "description": "Exact text to find (must match exactly)" },
      "new_text": { "type": "string", "description": "Replacement text" }
    },
    "required": ["path", "old_text", "new_text"]
  }
}
```

**Behavior:**
1. Read the file at `path`
2. Find `old_text` in the file content (exact byte match, not regex)
3. If found exactly once → replace with `new_text`, write file
4. If not found → error: "Could not find the exact text in {path}. The old_text must match exactly including all whitespace and newlines."
5. If found multiple times → error: "Found {n} matches for old_text in {path}. Please provide more context to uniquely identify the target."
6. Return: "Successfully edited {path}" with a brief diff summary (lines changed)

**Edge cases:**
- Empty `old_text` → error (would match everywhere)
- `old_text == new_text` → error (no-op)
- Binary files → error (same check as `read_file`)
- File doesn't exist → error
- `new_text` is empty → deletion (valid — removes the matched text)

### `read_file` Enhancements

Add optional `offset` and `limit` parameters:

```json
{
  "name": "read_file",
  "parameters": {
    "type": "object",
    "properties": {
      "path": { "type": "string" },
      "offset": { "type": "integer", "description": "Line number to start reading from (1-indexed)" },
      "limit": { "type": "integer", "description": "Maximum number of lines to return" }
    },
    "required": ["path"]
  }
}
```

**Behavior:**
- No `offset`/`limit` → current behavior (full file, subject to max_read_size)
- `offset` only → from line N to end
- `limit` only → first N lines
- Both → N lines starting at `offset`
- Response includes: `[Lines {start}-{end} of {total}]` header when partial
- 1-indexed (line 1 = first line)
- Out-of-range offset → return what's available with a note

## Security

### ProposalGateExecutor Integration

`edit_file` is a write tool and MUST be gated by ProposalGateExecutor:

1. Add `"edit_file"` to `WRITE_TOOLS` in `engine/crates/fx-kernel/src/proposal_gate.rs`
2. The gate intercepts `edit_file` calls, extracts the `path` parameter, classifies against tiers:
   - **Tier 3 (immutable):** Block entirely — return error
   - **Tier 2 (propose):** Generate a proposal diff instead of applying
   - **Tier 1 (allow):** Execute normally
3. Proposal format for `edit_file`: show the old_text → new_text diff in the proposal file

### Path Security
- Same `jailed_path()` validation as `read_file`/`write_file`
- Tilde expansion
- Symlink traversal check
- Path must resolve within working directory

### Self-Modify Policy
- Same tool-level secondary guard as `write_file` (defense-in-depth)
- `classify_path()` check before write

## Implementation

### Files to Modify

1. **`engine/crates/fx-tools/src/tools.rs`**
   - Add `EditFileArgs` struct (path, old_text, new_text)
   - Add `handle_edit_file()` method on `FawxToolExecutor`
   - Update `ReadFileArgs` with optional `offset: Option<usize>`, `limit: Option<usize>`
   - Update `handle_read_file()` for partial reads
   - Add `edit_file` to `fawx_tool_definitions()`
   - Update `read_file` definition with new parameters
   - Add match arm in `execute_call()` for `"edit_file"`
   - Wire self-modify policy check (same as write_file)

2. **`engine/crates/fx-kernel/src/proposal_gate.rs`**
   - Add `"edit_file"` to `WRITE_TOOLS` const array
   - No other changes needed — the gate already intercepts by tool name

3. **`engine/crates/fx-tools/src/skill_bridge.rs`**
   - `BuiltinToolsSkill` already wraps `FawxToolExecutor` — no changes needed if we add the tool definition to `fawx_tool_definitions()`

### Tests Required

**edit_file:**
- Exact match replacement succeeds
- Not-found returns descriptive error
- Multiple matches returns descriptive error with count
- Empty old_text rejected
- No-op (old == new) rejected
- Binary file rejected
- Path traversal blocked (jailed_path)
- Symlink outside jail blocked
- File doesn't exist → error
- Empty new_text (deletion) works
- Multiline old_text matching (exact whitespace)
- Tier 3 path blocked by tool-level guard
- Tier 2 path generates proposal

**read_file offset/limit:**
- Full read (no params) unchanged
- Offset only
- Limit only
- Offset + limit
- Offset past end of file → empty with note
- Limit larger than file → return what's available
- Line count header present in partial reads
- 1-indexed validation

**ProposalGateExecutor:**
- `edit_file` on Tier 3 path → blocked
- `edit_file` on Tier 2 path → proposal generated
- `edit_file` on Tier 1 path → passes through

## Size Estimate

~200-300 lines of new code + ~200 lines of tests. Single PR.

## Dependencies

None — builds on existing infrastructure (jailed_path, ProposalGateExecutor, FawxToolExecutor).
