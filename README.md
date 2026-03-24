# Fawx

[![CI](https://github.com/fawxai/fawx/actions/workflows/ci.yml/badge.svg)](https://github.com/fawxai/fawx/actions/workflows/ci.yml)
[![License: BSL 1.1](https://img.shields.io/badge/license-BSL%201.1-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)

Fawx is a local-first agentic engine. It runs on your machine, calls LLMs for reasoning, and executes tasks through a structured loop with built-in safety controls. One binary. Your data stays local. The agent works for you.

**1,800+ tests. 34 crates. WASM skill plugins. macOS and iOS apps.**

---

## Quick Start

```bash
# Build
git clone https://github.com/fawxai/fawx.git
cd fawx && cargo build --release

# Configure (interactive wizard)
./target/release/fawx setup

# Run
./target/release/fawx serve
```

Bring your own API key (Anthropic, OpenAI, or local models). Fawx never sends data anywhere except the LLM provider you choose.

---

## What It Does

Fawx runs an agentic loop: the agent reads your files, writes code, runs commands, searches memory, and calls external APIs. The kernel enforces safety boundaries so the agent operates within defined capabilities without per-action consent prompts.

```
You: "Find all TODO comments and summarize them"

Fawx: [search_text] Searching for TODO patterns...
      [read_file] Reading context around matches...
      [write_file] Writing summary to docs/todo-summary.md

      Found 23 TODOs across 12 files. Summary written to docs/todo-summary.md.
      Top areas: error handling (8), test coverage (6), documentation (5).
```

---

## Architecture

```
┌─────────────────────────────────────────────┐
│                  Shells                      │
│  TUI  ·  HTTP API  ·  macOS/iOS  ·  Telegram│
├─────────────────────────────────────────────┤
│              Agentic Loop                    │
│  perceive → plan → act → synthesize         │
├──────────┬──────────────┬───────────────────┤
│  Kernel  │    Tools     │    Loadable       │
│ (safety) │ (13 builtin) │  (WASM skills)    │
│          │              │                   │
│ cap gate │ file, shell  │ web search, fetch │
│ tripwire │ memory, git  │ weather, vision   │
│ ripcord  │ config, node │ tts, scheduler    │
├──────────┴──────────────┴───────────────────┤
│  LLM Providers  ·  Memory  ·  Embeddings    │
│  Claude · OpenAI · Local (llama.cpp)        │
└─────────────────────────────────────────────┘
```

### Kernel (immutable at runtime)

The kernel cannot be modified by the agent. It defines the boundaries:

- **Capability Gate** defines what the agent can and cannot do. Restricted actions get immediate structured denial. No modal prompts, no timeouts.
- **Tripwire** silently activates monitoring when the agent crosses soft boundaries within its capability space. The agent never knows.
- **Ripcord** atomically rolls back file and git operations from a tripwire point. Shell and API calls get audit-only treatment.
- **Budget Control** enforces token and iteration limits to prevent runaway loops.

### Loadable Layer (agent-editable)

WASM skills extend Fawx's capabilities. Each skill runs in a sandboxed WebAssembly environment with declared capabilities (network, storage). Skills are hot-reloadable and verified via Ed25519 signatures before loading.

### Shells (replaceable frontends)

- **TUI** for terminal users, with markdown rendering and streaming output
- **HTTP API** with SSE streaming for custom integrations
- **macOS and iOS apps** (native Swift)
- **Telegram** and webhook channels for remote access

---

## Built-in Tools

| Category | Tools |
|----------|-------|
| **Files** | `read_file`, `write_file`, `edit_file`, `list_directory`, `search_text` |
| **Shell** | `run_command`, `exec_background`, `exec_status` |
| **Memory** | `memory_write`, `memory_read`, `memory_list`, `memory_delete`, `memory_search` |
| **Git** | `git_status`, `git_diff`, `git_commit` |
| **Agent** | `spawn_agent`, `subagent_status`, `self_info`, `current_time` |

---

## WASM Skills

Skills are Rust crates compiled to WebAssembly. The [skill marketplace](https://github.com/fawxai) has ready-to-install skills. Building your own takes minutes:

```rust
#[no_mangle]
pub extern "C" fn run() {
    let input = get_input();
    let response = http_request("GET", &url, "", "");
    set_output(&result);
}
```

```bash
# Install a skill
fawx skill install fawxai/skill-web-search

# Or build your own
cargo generate fawxai/skill-template
cargo build --release --target wasm32-unknown-unknown
fawx skill install ./target/wasm32-unknown-unknown/release/my_skill.wasm
```

Available skills: [web search](https://github.com/fawxai/skill-brave-search) · [web fetch](https://github.com/fawxai/skill-web-fetch) · [scheduler](https://github.com/fawxai/skill-scheduler) · weather · vision · TTS · STT · browser · canvas

---

## LLM Providers

| Provider | Auth | Streaming | Tool Use | Thinking |
|----------|------|-----------|----------|----------|
| Anthropic Claude | API key, setup token | ✓ | ✓ | ✓ |
| OpenAI (GPT, Codex) | API key, OAuth PKCE | ✓ | ✓ | ✓ |
| Local (llama.cpp) | None | ✓ | ✓ | — |

Provider fallback with health tracking: if the primary provider fails, Fawx routes to the next healthy provider automatically.

---

## Security

Fawx takes a "boundaries, not checkpoints" approach. The kernel defines what the agent can do and enforces silently. No consent prompts by default.

**Capability Gate:** Tools operate within declared capability spaces. Restricted actions get immediate structured denial.

**Credential Store:** AES-256-GCM encryption at rest, per-operation unlock. Credentials are never held in memory longer than needed.

**WASM Sandboxing:** Skills run in isolated environments. Only declared capabilities are granted. The host API is the sole interface.

**Tripwire and Ripcord:** Invisible monitoring at soft boundaries, with atomic rollback for reversible operations. The agent operates naturally while the kernel watches.

---

## Configuration

All configuration lives in `~/.fawx/config.toml`:

```toml
[llm]
default_provider = "anthropic"

[llm.anthropic]
model = "claude-sonnet-4-20250514"

[server]
host = "127.0.0.1"
port = 8400

[memory]
enabled = true
embeddings_enabled = true

[permissions]
mode = "capability"  # or "prompt" for per-action approval
preset = "standard"  # open, standard, restricted
```

Run `fawx setup` for an interactive configuration wizard.

---

## Development

```bash
# Run all tests
cargo test --workspace

# Lint (zero warnings enforced)
cargo clippy --workspace --tests -- -D warnings

# Format
cargo fmt --all
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full development guide.

---

## Project Structure

```
fawx/
├── engine/crates/           # Rust crates
│   ├── fx-kernel/           # Loop, capability gate, tripwire, ripcord
│   ├── fx-llm/              # Anthropic, OpenAI, local providers
│   ├── fx-tools/            # Built-in tool implementations
│   ├── fx-loadable/         # WASM skill runtime, hot-reload, signatures
│   ├── fx-memory/           # Key-value store with decay + embeddings
│   ├── fx-auth/             # Credential store, OAuth, setup wizard
│   ├── fx-session/          # Multi-session management
│   ├── fx-journal/          # Reflective memory
│   ├── fx-telemetry/        # Opt-in telemetry with consent persistence
│   ├── fx-api/              # HTTP API, SSE streaming, handlers
│   ├── fx-cli/              # Engine binary, headless mode
│   └── ...                  # 20+ more crates
├── app/                     # macOS and iOS native app (Swift)
├── docs/                    # Architecture, specs, design decisions
└── scripts/                 # Build and deployment
```

---

## Roadmap

See [docs/roadmap.html](docs/roadmap.html) for the full roadmap. Current priorities:

1. **Open source launch** with skill marketplace
2. **Attachments** for images, files, and PDFs in the GUI
3. **OS-level enforcement** via Landlock, seccomp, network namespaces
4. **Signal flywheel** for earned autonomy through behavioral telemetry

---

## License

[Business Source License 1.1](LICENSE). Self-hosting for personal and internal business use is always permitted. The license converts to Apache 2.0 on 2030-03-23.

Skills in the [fawxai](https://github.com/fawxai) organization are Apache 2.0.
