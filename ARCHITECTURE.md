# Fawx Architecture

## Overview

Fawx is a TUI-first agentic engine built in Rust. The architecture separates **safety** (kernel — immutable at runtime) from **intelligence** (loadable — agent-editable), with shells as replaceable frontends.

## Workspace Structure

34 crates organized by architectural layer:

```
fawx/
├── engine/crates/
│   │
│   │── Kernel (immutable at runtime)
│   ├── fx-kernel/           # Loop orchestration, proposal gate, policy, ripcord, streaming
│   ├── fx-canary/           # Canary signals for safety monitoring
│   ├── fx-security/         # Security policy engine
│   ├── fx-auth/             # Credential store (AES-256-GCM), OAuth, setup wizard
│   ├── fawx-ripcord/        # Emergency shutdown binary
│   │
│   │── Intelligence Layer
│   ├── fx-llm/              # LLM providers (Anthropic, OpenAI, local), streaming, fallback
│   ├── fx-embeddings/       # Embedding model loading, cosine similarity
│   ├── fx-memory/           # Key-value memory with decay + embedding index
│   ├── fx-conversation/     # Conversation history persistence
│   ├── fx-journal/          # Reflective memory (journal_write, journal_search)
│   ├── fx-preprocess/       # Input preprocessing
│   ├── fx-agent/            # Agent reasoning and planning
│   │
│   │── Tool Layer
│   ├── fx-tools/            # 21 built-in tools (file, exec, memory, config, subagent)
│   ├── fx-propose/          # Proposal system for gated writes
│   ├── fx-scratchpad/       # Agent scratchpad for intermediate state
│   ├── fx-transactions/     # Multi-file atomic transactions
│   │
│   │── Loadable Layer (agent-editable)
│   ├── fx-loadable/         # WASM plugin system, hot-reload, signature verification
│   ├── fx-skills/           # WASM host API implementation (host_api_v1)
│   ├── fx-marketplace/      # Skill discovery and installation
│   │
│   │── Distribution Layer
│   ├── fx-fleet/            # Node registry for distributed operation
│   ├── fx-orchestrator/     # Task routing across nodes
│   ├── fx-subagent/         # Subagent spawning and lifecycle
│   ├── fx-session/          # Multi-session management
│   ├── fx-channel-telegram/ # Telegram bot channel
│   ├── fx-channel-webhook/  # Webhook channel
│   │
│   │── Engine Binary
│   ├── fx-cli/              # Main binary: HTTP server, headless mode, setup wizard
│   ├── fx-config/           # Configuration management, SIGHUP reload
│   │
│   │── Analysis & Improvement
│   ├── fx-analysis/         # Code analysis tools
│   ├── fx-improve/          # Self-improvement suggestions
│   ├── fx-decompose/        # Complexity decomposition
│   ├── fx-author/           # Authoring assistance
│   │
│   │── Foundation
│   ├── fx-core/             # Core types, traits, errors
│   ├── fx-storage/          # Encrypted persistence layer
│   ├── llama-cpp-sys/       # Rust FFI bindings to llama.cpp
│   └── fawx-test/           # Test utilities
│
├── tui/                     # Terminal UI (ratatui)
│   ├── fawx_backend.rs      # HTTP backend (connects to running engine)
│   ├── embedded_backend.rs  # Embedded backend (engine built-in)
│   ├── markdown_render.rs   # Terminal markdown rendering
│   └── app.rs               # UI state, input handling, key bindings
│
├── skills/                  # 8 WASM skills
│   ├── weather-skill/       # Weather via wttr.in / Open-Meteo
│   ├── vision-skill/        # Image analysis via Claude / GPT-4o
│   ├── tts-skill/           # Text-to-speech via OpenAI TTS
│   ├── stt-skill/           # Speech-to-text via OpenAI Whisper
│   ├── browser-skill/       # Web fetch, search, screenshot
│   ├── canvas-skill/        # Tables, charts, documents
│   ├── calculator-skill/    # Math evaluation
│   └── github-skill/        # GitHub API operations
│
└── docs/                    # Architecture, specs, design docs
```

## Key Design Decisions

### Kernel Immutability
The kernel (`fx-kernel`) cannot be modified by the agent at runtime. The proposal gate, policy engine, and ripcord are compiled-in constants. This prevents the agent from weakening its own safety constraints.

### Proposal Gate
Write tools (`write_file`, `edit_file`, `git_checkpoint`) are intercepted by the proposal gate before execution. Paths are classified into three tiers:
- **Tier 1**: Agent writes freely
- **Tier 2**: Requires proposal + user approval
- **Tier 3**: Unconditionally blocked (kernel source, auth crypto, CI config)

Tier 3 paths are `const` — they cannot change without recompilation.

### WASM Skill Sandboxing
Skills run in isolated WASM environments with no direct system access. The `host_api_v1` ABI is the sole interface:
- Capabilities are declared in `manifest.toml` and enforced at load time
- Binary data (audio, images) passes through a base64 sentinel encoding layer
- Skills are verified via Ed25519 signatures before loading
- Hot-reload replaces skills without engine restart

### Provider Fallback
`fx-llm` maintains health state per provider. When a provider fails (rate limit, network error, auth issue), requests automatically route to the next healthy provider. Health recovers over time.

### Memory Decay
Memories have a last-accessed timestamp. Reads call `touch()` to reset the timer. Unused memories decay and can be pruned. Semantic search via `memory_search` also touches results, keeping frequently-searched memories alive.

### Two-Binary Architecture
- `fawx` — The engine. Runs headless or as an HTTP server (`fawx serve`). Handles all LLM communication, tool execution, and WASM skill hosting.
- `fawx-tui` — The terminal UI. Connects to the engine via HTTP, or runs in embedded mode with the engine built-in.

This separation allows remote operation: run the engine on a server, connect the TUI from anywhere.

## Data Flow

```
User Input (TUI / HTTP API / Telegram)
    │
    ▼
Engine (fx-cli)
    │
    ├─► Perceive: parse input, load context
    ├─► Classify: determine intent
    ├─► Reason: LLM call with tools + context
    ├─► Plan: select tools from LLM response
    ├─► Act: execute tools (proposal gate intercepts writes)
    │     ├─► Built-in tools (fx-tools)
    │     ├─► WASM skills (fx-skills)
    │     └─► Subagent spawn (fx-subagent)
    ├─► Verify: check results, budget
    └─► Synthesize: format response
          │
          ▼
    Response (streamed via SSE or returned as JSON)
```

## HTTP API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/message` | POST | Send a message. Returns JSON or SSE stream based on `Accept` header. |
| `/health` | GET | Engine health check |

SSE streaming (`Accept: text/event-stream`) emits typed events: `phase_change`, `text_delta`, `tool_call_start`, `tool_call_complete`, `tool_result`, `done`.
