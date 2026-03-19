# Phase 1b Revised — TUI Unification + Data Integrity

**Scope change:** Phase 1b expanded from "slash commands server-side" to full TUI unification.
Pull forward Phase 3 embedded mode (#1271). Fix data corruption bugs. Remove legacy TUI.

---

## PR 1: Finish slash command fixes (PR #1272 amendments)

**Status:** PR #1272 open, APPROVE R4 on the DRY fixes. Smoke test found 3 more bugs.

### Remaining fixes
1. **`/config reload`** — returns config dump instead of confirmation; doesn't apply reloaded model to runtime
2. **`/improve` + `/analyze`** — verify they're wired server-side (fixer may have addressed this)
3. **`/auth` from HTTP** — should return "client-side command" message, not error

### NOT in this PR
- fawx-tui help text (that's in the TUI binary, addressed in PR 3)
- Signal/proposal corruption (separate PRs)

**Size:** ~50 lines of fixes
**Depends on:** nothing

---

## PR 2: Data integrity — signal + proposal corruption

### Bug A: Malformed signal JSONL (1274 bad lines on Joe's Mac)

**Root cause investigation needed.** The persist path (`SignalStore::persist`) serializes via `serde_json::to_string()` which shouldn't produce invalid JSON. Possible causes:
- Older Fawx version wrote signals with different `Signal` struct fields (added/removed fields between versions)
- `LoopStep` or `SignalKind` enum had different variant names in older versions (aliases exist for PascalCase → snake_case migration, suggesting this happened)
- Partial writes from crashes (append mode, no fsync)
- Encoding issues (non-UTF8 content in signal messages)

**Fix approach:**
1. Check a sample of malformed lines from Joe's Mac to identify the actual parse failure
2. Add missing serde aliases or `#[serde(other)]` for forward/backward compat
3. Add a `fawx signals cleanup` command to prune unparseable lines
4. Consider writing a schema version header to signal files

### Bug B: Malformed proposal markdown

**Root cause:** Writer uses dynamic fence length (`proposal_fence()` creates 4+ backtick fences when content contains triple backticks). Parser expects fences starting with ` ``` `. The parser DOES handle this (it matches `starts_with("```")`), but the error Joe hit was "legacy proposal diff fence missing" — meaning the file didn't have a `## Proposed Diff` section at all, or the section structure was wrong.

**Fix approach:**
1. Make parser tolerant of missing/malformed sections — return partial data with warnings (#1273)
2. Validate proposal structure at write time (fail fast if content would produce unparseable output)
3. Add `fawx proposals cleanup` to remove stale/broken proposals

**Size:** ~200 lines across fx-memory, fx-propose, fx-cli
**Depends on:** nothing (parallel with PR 1)

---

## PR 3: fawx-tui thin client cleanup

The `tui/` directory in the monorepo IS the canonical fawx-tui. The separate `abbudjoe/fawxtui` repo is stale and diverged — archive it.

### Changes
1. **Remove hardcoded command handling from fawx-tui** — no local `/help`, `/auth`, `/model`, `/config`, `/memory`. Everything goes to server via `/message`.
2. **Keep only truly local commands** — `/quit` (exit), `/clear` (terminal clear)
3. **Update help text** — on startup, fetch `/help` from server instead of showing hardcoded list
4. **Remove `credential_reader.rs` and `local_auth.rs`** — auth handled by server. Bearer token comes from env var or config file only (no credential store decryption in TUI).
5. **Archive `abbudjoe/fawxtui`** — add deprecation notice pointing to monorepo

**Size:** ~300 lines (mostly deletions)
**Depends on:** PR 1 (server-side commands must work first)

---

## PR 4: Embedded mode — standalone ratatui TUI (#1271)

### Architecture
`fawx-tui` gets an `EmbeddedBackend` that runs the engine in-process:

```
fawx-tui              → FawxBackend (HTTP client) → fawx serve
fawx-tui --embedded   → EmbeddedBackend (in-process) → engine directly
```

### Implementation
1. **`EmbeddedBackend`** — implements same trait as `FawxBackend`. Wraps `HeadlessApp` (or a subset). Calls `process_message()` directly.
2. **Shared `Backend` trait** — extract from `FawxBackend`. Both backends implement it.
3. **Engine dependencies** — `fawx-tui` Cargo.toml adds `fx-cli` (or relevant engine crates) as optional dependency behind `embedded` feature flag.
4. **CLI flag** — `fawx-tui --embedded` selects `EmbeddedBackend`. Default remains HTTP.
5. **Slash commands in embedded mode** — route through `execute_command()` from `commands/slash.rs`.

### Streaming consideration
- **Phase 1b scope:** No streaming. Full response, then render. Same as current HTTP mode.
- **Future:** Add streaming via async channel between engine loop and TUI render loop.

**Size:** ~400-500 lines
**Depends on:** PR 1 (shared command infrastructure), PR 3 (clean TUI codebase)

---

## PR 5: Remove legacy TUI

After embedded mode works:
1. Delete `engine/crates/fx-cli/src/tui.rs` (3,600+ lines)
2. Remove `fawx tui-legacy` subcommand from `main.rs`
3. Remove TUI-only dependencies from fx-cli (crossterm raw mode, termimad, etc.)
4. Update `fawx tui` to just launch `fawx-tui` (already done in PR #1262)
5. Update docs, help text, setup wizard output

**Size:** ~3,600 lines deleted, ~50 lines changed
**Depends on:** PR 4 (embedded mode must work as replacement)

---

## Build Order

```
PR 1 (slash fixes) ──→ PR 3 (thin client) ──→ PR 4 (embedded) ──→ PR 5 (remove legacy)
                    ↗
PR 2 (data integrity)  [parallel with PR 1]
```

## Acceptance Criteria

- [ ] `fawx-tui` works standalone (--embedded) with full slash command support
- [ ] `fawx-tui` works with server (default) with full slash command support
- [ ] `fawx tui-legacy` removed
- [ ] No malformed signal/proposal data written
- [ ] Graceful handling of existing malformed data
- [ ] `abbudjoe/fawxtui` archived
- [ ] One TUI binary, two modes, full parity
