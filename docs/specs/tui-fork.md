# Fawx TUI — Codex CLI Fork Specification

**Status:** Draft
**Location:** `fawx/tui/` (in-repo, alongside `engine/`)
**Source:** Fork of `openai/codex` → `codex-rs/tui/`
**License:** Apache-2.0 (same as source)

---

## Goal

Fork the Codex CLI TUI crate into the Fawx repo, replacing the `codex-core` backend with an HTTP client adapter that talks to `fawx serve --http` on `localhost:8400`. The result is a production-quality terminal UI powered by Fawx's engine.

`fawx serve --http` always binds `127.0.0.1:8400` for local clients. When a Tailscale interface is available it also binds the Tailscale address on the same port, but localhost remains the guaranteed endpoint.

## Phase 1 — Basic Chat (MVP)

Get the TUI compiling and running with basic chat functionality:
- User types a message
- Message sent to Fawx engine via HTTP POST `/message`
- Streamed response rendered with full markdown support
- Fawx branding (colors, name, hero art)

### What to do

1. **Clone source:** `git clone https://github.com/openai/codex.git /tmp/codex-source`

2. **Copy TUI crate:** Copy `codex-rs/tui/` → `fawx/tui/`

3. **Copy required shared crates** from `codex-rs/` that the TUI depends on:
   - `codex-ansi-escape` — ANSI escape handling
   - `codex-utils-string` — string utilities
   - `codex-utils-elapsed` — time formatting
   - `codex-utils-absolute-path` — path utilities
   - Any other `codex-*` utility crates that are pure helpers (no codex-core dependency)
   - Place these in `fawx/tui/vendor/` or adapt inline

4. **Create backend adapter:** `fawx/tui/src/backend.rs` (or `fawx/tui/src/fawx_backend/`)
   - HTTP client that talks to `http://localhost:8400`
   - `POST /message` — send user message, receive streamed response
   - `GET /health` — check engine is running
   - `GET /status` — get engine status
   - Must implement whatever trait/interface the TUI expects from codex-core
   - Use `reqwest` for HTTP, `tokio` for async (both already in codex-tui deps)

5. **Replace codex-core dependency:**
   - Remove `codex-core` from `Cargo.toml`
   - Replace all `use codex_core::*` imports with adapter types
   - Key types to adapt: `Op` (operations/events from backend), message types, tool call types
   - Stub out what's not needed yet (MCP, sandbox, etc.) with no-op implementations

6. **Strip OpenAI auth:**
   - Remove `codex-login`, `codex-chatgpt` dependencies
   - Replace onboarding flow with simple "connecting to Fawx engine..." screen
   - No API key needed — the engine handles auth

7. **Rebrand:**
   - Binary name: `fawx` (or `fawx-tui` if needed to avoid conflict with engine binary)
   - Window title, status bar: "Fawx" not "Codex"
   - Color scheme: keep dark theme, adjust accent colors
   - Remove OpenAI logos/references

8. **Get it compiling:** `cargo build -p fawx-tui`

9. **Test basic flow:**
   - Start engine: `fawx serve --http` (in another terminal)
   - Run TUI: `cargo run -p fawx-tui`
   - Type a message, see streamed response with markdown rendering

### What to preserve (don't delete)

Keep all of these even if not wired up yet — they'll be connected in Phase 2+:
- `approval_overlay.rs` — will wire to ProposalGateExecutor
- `diff_render.rs` — will wire to GitSkill
- `file_search.rs` — will wire to search_text tool
- `multi_agents.rs` — will wire to fx-fleet orchestrator
- `voice.rs` — future feature
- `markdown_render.rs`, `markdown_stream.rs` — core rendering (use immediately)
- `streaming/` — core streaming (use immediately)
- `wrapping.rs` — core text wrapping (use immediately)
- All snapshot tests — update to match rebranded output

### What to stub/disable (for now)

- MCP server/client — return no-op
- Sandbox management — not applicable (engine handles this)
- ChatGPT OAuth — removed entirely
- Model migration — not applicable
- Realtime audio — stub (Phase 2+)
- Backend client reconnection — simple HTTP, no websocket

---

## Architecture

```
┌─────────────────────────────────────┐
│           fawx-tui (Rust)           │
│  ┌─────────┐  ┌──────────────────┐  │
│  │ Markdown │  │ Approval Overlay │  │
│  │ Renderer │  │   (Phase 2)      │  │
│  ├─────────┤  ├──────────────────┤  │
│  │Streaming │  │  Diff Render     │  │
│  │ Chunking │  │   (Phase 2)      │  │
│  ├─────────┤  ├──────────────────┤  │
│  │   Chat   │  │  Fleet Status    │  │
│  │  Widget  │  │   (Phase 2)      │  │
│  └────┬─────┘  └──────────────────┘  │
│       │                               │
│  ┌────▼──────────────────────────┐   │
│  │     Fawx Backend Adapter      │   │
│  │  HTTP client → localhost:8400 │   │
│  └───────────────────────────────┘   │
└─────────────────────────────────────┘
            │ HTTP
┌───────────▼─────────────────────────┐
│      fawx serve --http (Rust)       │
│  Engine: LLM + Tools + Memory +     │
│  Policy + WASM Skills + Security    │
└─────────────────────────────────────┘
```

---

## Key Files to Understand First

Before modifying anything, read these to understand the architecture:

1. `codex-rs/tui/src/lib.rs` (55K) — TUI initialization, event loop
2. `codex-rs/tui/src/chatwidget.rs` (347K) — main chat widget, message rendering
3. `codex-rs/tui/src/app.rs` (265K) — application state machine
4. `codex-rs/core/src/lib.rs` — codex-core's public interface (what the TUI calls)
5. `codex-rs/tui/src/streaming/` — how streaming responses are rendered

The adapter needs to provide whatever `codex-core` exposes that the TUI actually uses. Start by grepping for `use codex_core` in the TUI crate to map the dependency surface.

---

## HTTP API Contract (Fawx Engine)

The TUI talks to the engine via these endpoints:

### POST /message
```json
Request:  { "content": "user message text" }
Response: Server-Sent Events (SSE) stream
  data: {"type": "text_delta", "content": "partial "}
  data: {"type": "text_delta", "content": "response"}
  data: {"type": "tool_use", "name": "read_file", "arguments": {"path": "src/main.rs"}}
  data: {"type": "tool_result", "content": "file contents..."}
  data: {"type": "done", "stop_reason": "end_turn"}
```

### GET /health
```json
Response: { "status": "ok" }
```

### GET /status
```json
Response: { "model": "claude-opus-4-6", "memory_entries": 12, "tools": [...] }
```

Note: The exact HTTP wire format depends on what `fawx serve --http` actually sends. Read `engine/crates/fx-cli/src/http_serve.rs` to understand the current implementation before building the adapter.

---

## Repo Structure After Fork

```
fawx/
├── engine/          ← existing Rust engine (fx-* crates)
├── tui/             ← NEW: forked Codex TUI
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── fawx_backend/    ← NEW: HTTP adapter
│   │   ├── chatwidget.rs    ← forked, adapted
│   │   ├── app.rs           ← forked, adapted
│   │   ├── markdown_render.rs  ← mostly unchanged
│   │   ├── streaming/       ← mostly unchanged
│   │   └── ...
│   ├── vendor/      ← copied codex-* utility crates
│   └── tests/
├── docs/
└── .github/
```

---

## Success Criteria (Phase 1)

1. `cargo build -p fawx-tui` compiles clean
2. Running `fawx-tui` connects to `fawx serve --http` on localhost:8400
3. User can type a message and see a streamed response
4. Markdown renders correctly (headers, code blocks, bold, italic, lists)
5. Ctrl+C exits cleanly
6. No OpenAI/Codex branding visible
7. All preserved code compiles (even if not wired up yet)

---

## Phase 2+ Roadmap (not in scope now)

- Wire approval overlay → ProposalGateExecutor approval flow
- Wire diff render → GitSkill file modification display
- Wire file search → search_text/list_directory tools
- Wire multi-agent view → fx-fleet orchestrator status
- Tool call rendering (show tool name, arguments, results)
- Memory display (show memory entries, cross-session recall)
- Signal indicators (friction, success, thinking)
- `/model` command integration
- Voice input integration
- Session resume/picker
