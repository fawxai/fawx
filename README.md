# Fawx

[![CI](https://github.com/abbudjoe/fawx/actions/workflows/ci.yml/badge.svg)](https://github.com/abbudjoe/fawx/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.83%2B-orange.svg)](https://www.rust-lang.org/)

Fawx is a TUI-first agentic engine. It runs locally, talks to cloud LLMs for reasoning, and executes tasks through a structured loop with built-in safety controls. Think of it as an operating system for AI agents — a kernel that enforces safety, a loadable layer that provides intelligence, and a plugin system for extensibility.

**2,200+ tests. 126k lines of Rust. 34 crates. 8 WASM skills. One binary.**

---

## What It Does

Fawx runs a 7-step agentic loop: perceive → classify → reason → plan → act → verify → synthesize. The agent reads files, writes code, runs commands, searches memory, and calls external APIs — all within a safety framework that prevents unchecked writes to critical paths.

```
You: "Find all TODO comments in the codebase and create a summary"

Fawx: [read_file] Reading project structure...
      [search_text] Searching for TODO patterns...
      [write_file] Writing summary to docs/todo-summary.md
      
      Found 23 TODOs across 12 files. Summary written to docs/todo-summary.md.
      Top areas: error handling (8), test coverage (6), documentation (5).
```

---

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Shells                            │
│  ┌──────────┐  ┌──────────┐  ┌───────────────────┐  │
│  │  fawx-tui│  │ HTTP API │  │ Telegram Channel  │  │
│  └────┬─────┘  └────┬─────┘  └────────┬──────────┘  │
│       └──────────────┼─────────────────┘             │
├──────────────────────┼───────────────────────────────┤
│              Engine (fx-cli)                         │
│  ┌───────────────────┼───────────────────────────┐   │
│  │            7-Step Agentic Loop                 │   │
│  │  perceive → classify → reason → plan →        │   │
│  │  act → verify → synthesize                    │   │
│  └───────────────────┬───────────────────────────┘   │
├──────────────────────┼───────────────────────────────┤
│  ┌─────────┐  ┌──────┴──────┐  ┌─────────────────┐  │
│  │ Kernel  │  │   Tools     │  │   Loadable       │  │
│  │(immutable)│ │ (21 built-in)│ │ (WASM skills)   │  │
│  │         │  │             │  │                  │  │
│  │ proposal│  │ read/write  │  │ weather,vision   │  │
│  │ gate    │  │ exec/bg     │  │ tts,stt,browser  │  │
│  │ policy  │  │ memory      │  │ canvas,scheduler │  │
│  │ ripcord │  │ config      │  │                  │  │
│  └─────────┘  └─────────────┘  └─────────────────┘  │
├──────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌────────────────────┐  │
│  │ fx-llm   │  │fx-memory │  │ fx-embeddings     │  │
│  │ Claude   │  │ JSON +   │  │ semantic search   │  │
│  │ OpenAI   │  │ decay    │  │ cosine similarity │  │
│  │ local    │  │          │  │                   │  │
│  └──────────┘  └──────────┘  └────────────────────┘  │
└──────────────────────────────────────────────────────┘
```

### Kernel (Immutable at Runtime)

The kernel cannot be modified by the agent. It enforces:

- **Proposal Gate** — Write operations to protected paths require explicit approval. Tier 3 paths (kernel source, auth crypto, CI config) are unconditionally blocked.
- **Policy Engine** — Capability-based access control for tools and actions.
- **Ripcord** — Emergency shutdown triggered by canary signal violations.
- **Budget Control** — Token and iteration limits prevent runaway loops.

### Loadable Layer (Agent-Editable)

Everything the agent can modify lives here:

- **WASM Skills** — Sandboxed plugins with declared capabilities (network, storage). Hot-reloadable with cryptographic signature verification.
- **Strategies** — Agent behavior configuration.
- **Templates** — Prompt templates for different tasks.

### Shells (Replaceable Frontends)

- **fawx-tui** — Terminal interface with markdown rendering, streaming output, and keyboard navigation. Runs in HTTP mode (connecting to a running engine) or embedded mode (engine built-in).
- **HTTP API** — RESTful API with SSE streaming. `POST /message` for requests, `Accept: text/event-stream` for real-time token streaming.
- **Channels** — Telegram and webhook integrations for remote access.

---

## Built-in Tools

### File Operations
| Tool | Description |
|------|-------------|
| `read_file` | Read files with optional offset/limit for large files |
| `write_file` | Create or overwrite files (proposal-gated on protected paths) |
| `edit_file` | Surgical find-and-replace edits |
| `list_directory` | Browse directory contents |
| `search_text` | Regex search across files |

### Execution
| Tool | Description |
|------|-------------|
| `run_command` | Execute shell commands |
| `exec_background` | Start long-running processes |
| `exec_status` | Check background process status |
| `exec_kill` | Terminate background processes |

### Memory
| Tool | Description |
|------|-------------|
| `memory_write` | Store key-value memories with automatic embedding indexing |
| `memory_read` | Retrieve memories (touch resets decay timer) |
| `memory_list` | List all stored memories |
| `memory_delete` | Remove memories (also removes from embedding index) |
| `memory_search` | Semantic search via embeddings, keyword fallback |

### Agent Control
| Tool | Description |
|------|-------------|
| `spawn_agent` | Launch subagent for parallel work |
| `subagent_status` | Check subagent progress |
| `self_info` | Agent identity and capabilities |
| `current_time` | Current timestamp |

### Configuration
| Tool | Description |
|------|-------------|
| `config_get` | Read configuration values |
| `config_set` | Update configuration |
| `fawx_status` | Engine status and health |
| `fawx_restart` | Graceful restart with SIGHUP |

---

## WASM Skills

Skills are sandboxed WebAssembly plugins that extend Fawx's capabilities. Each skill declares its required capabilities (network, storage) and communicates through a versioned host API.

| Skill | Tools | Capabilities |
|-------|-------|-------------|
| **Weather** | `get_weather` | network |
| **Vision** | `analyze_image` | network, storage |
| **TTS** | `text_to_speech` | network, storage |
| **STT** | `speech_to_text` | network, storage |
| **Browser** | `web_fetch`, `web_search`, `web_screenshot` | network, storage |
| **Canvas** | `render_table`, `render_chart`, `render_document` | storage |
| **Calculator** | `calculate` | — |
| **Scheduler** | `scheduler` (add/remove/list/check) | storage |

### Skill Development

Skills are Rust crates compiled to `wasm32-unknown-unknown`:

```rust
#[no_mangle]
pub extern "C" fn run() {
    let input = get_input();           // JSON from host
    let api_key = kv_get("api_key");   // Secure credential access
    let response = http_request(       // Network call (if capability granted)
        "GET", &url, "", ""
    );
    set_output(&result);               // Return to host
}
```

```bash
# Build
cargo build --release --target wasm32-unknown-unknown

# Install
fawx skill install path/to/skill.wasm

# Skills are verified via Ed25519 signatures before loading
fawx skill sign path/to/skill.wasm --key path/to/key
```

### Host API (v1)

Skills access host functions through a stable ABI:

| Function | Description |
|----------|-------------|
| `get_input` | Receive JSON input from engine |
| `set_output` | Return JSON result to engine |
| `log` | Structured logging (debug/info/warn/error) |
| `http_request` | HTTP calls (requires `network` capability) |
| `kv_get` / `kv_set` | Key-value storage (requires `storage` capability) |

Binary data (audio, images) passes through a base64 sentinel encoding layer that handles the string-based ABI transparently.

---

## LLM Providers

| Provider | Auth | Features |
|----------|------|----------|
| **Anthropic Claude** | API key, setup token | Streaming, tool use, thinking |
| **OpenAI** | API key, PKCE OAuth | Streaming, tool use |
| **Local (llama.cpp)** | — | On-device inference, GGUF models |

Provider fallback with health tracking: if the primary provider fails, Fawx automatically routes to the next healthy provider.

---

## Streaming

Fawx streams at three layers:

1. **Provider Layer** — Token-by-token streaming from LLMs via SSE/WebSocket
2. **Engine Layer** — Phase change events, tool call lifecycle, completion events
3. **HTTP Layer** — SSE endpoint (`Accept: text/event-stream`) with typed events:

```
event: phase_change
data: {"phase":"reason"}

event: text_delta
data: {"content":"Here's what I found..."}

event: tool_call_start
data: {"name":"search_text","id":"call_1"}

event: tool_call_complete
data: {"name":"search_text","id":"call_1"}

event: done
data: {"reason":"complete"}
```

---

## Memory & Embeddings

Fawx has a two-tier memory system:

- **Key-Value Store** — JSON-backed with automatic decay (unused memories fade). Every read touches the key, resetting its decay timer.
- **Embedding Index** — Optional semantic search layer. When an embedding model is available, memories are automatically indexed on write and searchable by meaning via `memory_search`. Falls back to keyword search when embeddings are unavailable.

The embedding index uses a custom binary serialization format with corruption detection (magic bytes, version, entry count verification, dimension validation).

---

## Security Model

### Proposal Gate

Write operations are classified by path sensitivity:

| Tier | Paths | Policy |
|------|-------|--------|
| **Tier 1** | General files | Agent writes freely |
| **Tier 2** | Config, scripts | Requires proposal + approval |
| **Tier 3** | Kernel source, auth crypto, CI | **Unconditionally blocked** |

The proposal gate intercepts `write_file`, `edit_file`, and `git_checkpoint` before execution. Tier 3 paths are compiled as constants — they cannot be modified at runtime.

### Credential Store

- AES-256-GCM encryption at rest
- Per-operation unlock (credentials aren't held in memory)
- Setup wizard for initial configuration (`fawx setup`)
- Bearer auth, API key, and OAuth (PKCE) flows supported

### WASM Sandboxing

- Skills run in isolated WASM environments
- Only declared capabilities are granted
- Host API is the sole interface — no direct system access
- Ed25519 signature verification before loading
- Hot-reload replaces skills without engine restart

### Ripcord

Emergency shutdown system triggered by canary signal violations. When the agent's behavior diverges from expected patterns, the ripcord kills the process. This is a last-resort safety mechanism — the proposal gate and policy engine handle normal enforcement.

---

## Getting Started

### Prerequisites
- Rust 1.83+ ([rustup.rs](https://rustup.rs/))
- C++ compiler (for llama.cpp FFI)

### Build

```bash
git clone https://github.com/abbudjoe/fawx.git
cd fawx

# Build engine + TUI
cargo build --release

# Build WASM skills
cd skills && ./build.sh && cd ..
```

### Setup

```bash
# Interactive setup wizard — configure providers and credentials
./target/release/fawx setup
```

### Run

```bash
# Start the engine (HTTP API on localhost:8400)
./target/release/fawx serve

# In another terminal, start the TUI
./target/release/fawx-tui

# Or run in embedded mode (engine + TUI in one process)
./target/release/fawx-tui --embedded
```

### Configuration

Configuration lives in `~/.fawx/config.toml`:

```toml
[llm]
default_provider = "anthropic"

[llm.anthropic]
model = "claude-sonnet-4-20250514"

[llm.openai]
model = "gpt-4o"

[server]
host = "127.0.0.1"
port = 8400

[memory]
enabled = true
embeddings_enabled = true

[logging]
level = "info"
persistent = true
```

---

## Testing

```bash
# Run all tests (2,200+)
cargo test --workspace

# Run tests for a specific crate
cargo test -p fx-tools
cargo test -p fx-kernel
cargo test -p fx-llm

# Clippy (zero warnings enforced)
cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --check

# Build WASM skills and run their tests
cd skills/weather-skill && cargo test
cd skills/browser-skill && cargo test
# ... etc
```

---

## Project Structure

```
fawx/
├── engine/crates/          # 34 Rust crates
│   ├── fx-kernel/          # Loop orchestration, proposal gate, policy, ripcord
│   ├── fx-llm/             # LLM providers (Anthropic, OpenAI, local)
│   ├── fx-tools/           # 21 built-in tools
│   ├── fx-memory/          # Key-value memory with decay + embedding index
│   ├── fx-embeddings/      # Embedding model loading + cosine similarity
│   ├── fx-skills/          # WASM skill runtime + host API
│   ├── fx-loadable/        # Plugin system, hot-reload, signature verification
│   ├── fx-cli/             # Engine binary, HTTP server, headless mode
│   ├── fx-auth/            # Credential store (AES-256-GCM), OAuth, setup wizard
│   ├── fx-config/          # Configuration management, SIGHUP reload
│   ├── fx-session/         # Multi-session management
│   ├── fx-subagent/        # Subagent spawning and lifecycle
│   ├── fx-conversation/    # Conversation persistence
│   ├── fx-journal/         # Reflective memory (journal_write, journal_search)
│   ├── fx-channel-telegram/# Telegram bot channel
│   ├── fx-channel-webhook/ # Webhook channel
│   ├── fx-fleet/           # Node registry
│   ├── fx-orchestrator/    # Task routing
│   ├── fx-propose/         # Proposal system for gated writes
│   ├── fx-canary/          # Canary signals for ripcord
│   ├── fx-security/        # Security policy engine
│   └── ...                 # Core types, preprocessing, analysis, etc.
├── tui/                    # Terminal UI (ratatui-based)
├── skills/                 # 8 WASM skills
│   ├── weather-skill/
│   ├── vision-skill/
│   ├── tts-skill/
│   ├── stt-skill/
│   ├── browser-skill/
│   ├── canvas-skill/
│   ├── calculator-skill/
│   └── github-skill/
├── docs/                   # Architecture, specs, design docs
└── scripts/                # Build and deployment scripts
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.
