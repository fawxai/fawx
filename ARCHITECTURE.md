# Citros Architecture

## Overview

Citros is an AI-native phone agent designed across three horizons:
- **Phase 0.5**: Mac Mini pre-PoC (validate cognitive pipeline)
- **Horizon 1**: Android PoC (Rust daemon on rooted Pixel 8a)
- **Horizon 2**: AI-Native OS (custom Linux-based operating system)
- **Horizon 3**: Purpose-built hardware

This document describes the crate architecture for Phase 0.5 and Horizon 1. For complete architectural details, design decisions, and future roadmap, see **[SPEC.md](docs/SPEC.md)**.

## Workspace Structure

Citros is organized as a Cargo workspace with 12 crates:

```
citros/
├── Cargo.toml                 # Workspace root
├── crates/
│   ├── ct-core/              # [100% reuse] Core types, config, event bus, errors
│   ├── ct-agent/             # [100% reuse] Agent reasoning loop, orchestrator
│   ├── ct-llm/               # [100% reuse] LLM provider abstraction (local+cloud)
│   ├── ct-phone/             # [0% reuse]  Android UI puppeting (PoC only)
│   ├── ct-phone-sim/         # [Pre-PoC]   Mock phone for testing without hardware
│   ├── ct-voice/             # [95% reuse]  Voice I/O, STT, TTS, wake word
│   ├── ct-security/          # [90% reuse]  Capabilities, crypto, policy, audit
│   ├── ct-skills/            # [100% reuse] WASM skill runtime
│   ├── ct-sync/              # [100% reuse] Cloud sync client (outbound-only)
│   ├── ct-storage/           # [100% reuse] Encrypted key-value store
│   ├── ct-sensors/           # [80% reuse]  Device state monitoring
│   └── ct-cli/               # [100% reuse] CLI management interface
└── ffi/                       # (Future) FFI bindings for llama.cpp, whisper.cpp
```

## Crate Responsibilities

### ct-core (100% reuse)

**Purpose**: Foundation crate providing types, configuration, and utilities used by all other crates.

**Modules**:
- `config.rs` - Configuration loading and validation (JSON5)
- `types.rs` - Shared types: `UserInput`, `Intent`, `ActionPlan`, `ActionStep`, `ActionResult`, `AgentResponse`
- `event.rs` - Event bus using `tokio::sync::broadcast` for inter-crate communication
- `message.rs` - Internal message types
- `error.rs` - Error taxonomy: `CoreError`, `LlmError`, `StorageError`, `SecurityError`, `SkillError`, `PhoneError`

**Key Traits**:
- `PhoneActions` - Abstraction for phone control (implemented by `ct-phone` and `ct-phone-sim`)

**Dependencies**: `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`

---

### ct-agent (100% reuse)

**Purpose**: Core agent logic - orchestrates the perception → cognition → action loop.

**Responsibilities**:
- Receive user input (voice, text, notification, scheduled)
- Classify intent (via `ct-llm`)
- Route to local or cloud LLM based on complexity
- Generate action plans
- Execute plans against `PhoneActions` trait
- Maintain conversational context and short-term memory

**Future Modules** (Epic 4):
- `orchestrator.rs` - Main agent loop
- `intent.rs` - Intent classification
- `planner.rs` - Action plan generation
- `executor.rs` - Plan execution with verification
- `memory.rs` - Conversation history and context management

**Dependencies**: `ct-core`, `tokio`, `tracing`

---

### ct-llm (100% reuse)

**Purpose**: LLM provider abstraction for both local (llama.cpp) and cloud (Claude) inference.

**Future Modules** (Epic 2-3):
- `traits.rs` - `LlmProvider` trait
- `local.rs` - llama.cpp FFI wrapper for local models (Gemma 3n)
- `cloud.rs` - Claude API client with streaming and tool use
- `router.rs` - Confidence-based routing between local and cloud
- `prompts/` - System prompts for intent classification, planning, conversation

**Dependencies**: `ct-core`, `tokio`, `tracing` (+ `reqwest` for cloud, FFI for local)

**Routing Logic**:
- Simple commands (launch app, settings) → local model (fast, private)
- Complex tasks (multi-step planning, reasoning) → cloud model (powerful, accurate)
- Low confidence from local → escalate to cloud
- Fallback: local failure → cloud, cloud unavailable → local-only mode

---

### ct-phone (0% reuse - PoC only)

**Purpose**: Android-specific phone control via touch injection, screen capture, and accessibility services.

**This is disposable scaffolding.** In Horizon 2, the OS provides native phone control APIs. This crate exists only to validate the agent on Android in Horizon 1.

**Future Implementation** (Horizon 1 Phase 2):
- `input.rs` - Touch injection via `/dev/input`
- `screen.rs` - Screen capture via `screencap` or `SurfaceFlinger`
- `ui_tree.rs` - Accessibility service integration
- `apps.rs` - App management via `am`/`pm`
- `gestures.rs` - High-level gestures (tap, swipe, pinch)

**Implements**: `PhoneActions` trait from `ct-core`

---

### ct-phone-sim (Pre-PoC only)

**Purpose**: Simulated phone environment for testing the agent without hardware.

**Implements**: `PhoneActions` trait with mock responses. Maintains in-memory state for apps, screen, notifications.

**Use Case**: Validates agent reasoning, action planning, and policy enforcement on Mac Mini before purchasing Android hardware.

**Modules**:
- `lib.rs` - `SimulatedPhone` struct with `PhoneActions` implementation

**Status**: Fully functional for Phase 0.5. Replaced by `ct-phone` in Horizon 1.

---

### ct-voice (95% reuse)

**Purpose**: Voice input (STT, wake word) and output (TTS).

**Future Modules** (Horizon 1 Phase 2):
- `wake_word.rs` - Porcupine wake word detection
- `stt.rs` - whisper.cpp for local STT + Android fallback
- `tts.rs` - On-device TTS + optional cloud (ElevenLabs)
- `audio.rs` - Audio capture and playback (CPAL / Android APIs)

**Reuse**: 95% (audio APIs differ slightly between macOS, Android, and Horizon 2)

---

### ct-security (90% reuse)

**Purpose**: Security boundary between agent plans and device execution.

**Modules** (Epic 5):
- `policy.rs` - Action policy engine (ALLOW/CONFIRM/DENY rules)
- `capabilities.rs` - Linux capability dropping
- `crypto.rs` - AES-256-GCM encryption (via `ring`)
- `keystore.rs` - Hardware keystore integration
- `audit.rs` - Append-only tamper-evident audit log
- `verify.rs` - Skill signature verification (Ed25519)

**Policy Categories**:
- **ALLOW**: Launch app, read screen, search, navigation
- **CONFIRM**: Send message, modify contacts, change settings
- **DENY**: Factory reset, disable policy, financial transactions (v1)
- **RATE-LIMITED**: >30 actions/min, >5 messages/2min

**Reuse**: 90% (keystore APIs differ between Android and Horizon 2)

---

### ct-skills (100% reuse)

**Purpose**: WASM skill runtime with capability enforcement.

**Future Modules** (Epic 8):
- `runtime.rs` - wasmtime host and instance management
- `loader.rs` - Skill loading and signature verification
- `capabilities.rs` - Capability grants (network domains, storage quota, phone actions)
- `host_api.rs` - Functions exported to WASM guests (host API v1)
- `manifest.rs` - Skill manifest format (TOML)

**Host API** (exported to skills):
- `host_log(level, msg)`
- `host_http_get(url) -> response` (domain-restricted)
- `host_storage_get(key) -> value` (namespaced per skill)
- `host_storage_set(key, value)` (quota-enforced)
- `host_get_location() -> latlon` (if capability granted)

**Reuse**: 100% (WASM is architecture-neutral)

---

### ct-sync (100% reuse)

**Purpose**: Cloud sync client for encrypted backups, state sync, and remote command polling.

**All connections are outbound-only.** The phone initiates; the cloud responds. Zero inbound ports.

**Future Modules** (Epic 9):
- `client.rs` - HTTPS client with mTLS and certificate pinning
- `backup.rs` - Encrypted state backup/restore
- `command_queue.rs` - Poll remote command queue
- `skill_updates.rs` - OTA skill updates

---

### ct-storage (100% reuse)

**Purpose**: Encrypted persistent storage for credentials, conversation history, preferences.

**Future Modules** (Epic 6):
- `kv.rs` - redb key-value store with transparent encryption
- `history.rs` - Conversation history management
- `credentials.rs` - API keys, tokens
- `preferences.rs` - User preferences and learned patterns

**Encryption**: AES-256-GCM via `ring`, key derived from user PIN + device ID (HKDF-SHA256)

---

### ct-sensors (80% reuse)

**Purpose**: Device state monitoring (notifications, location, connectivity, battery).

**Future Modules** (Horizon 1 Phase 4):
- `notifications.rs` - Notification listener
- `location.rs` - GPS/network location
- `connectivity.rs` - WiFi/cellular state
- `battery.rs` - Battery and power state
- `triggers.rs` - Event-based trigger engine

**Reuse**: 80% (notification APIs differ between Android and Horizon 2)

---

### ct-cli (100% reuse)

**Purpose**: Command-line management interface.

**Commands**:
- `citros start` / `citros stop` - Daemon control
- `citros chat` - Interactive REPL
- `citros doctor` - Diagnostics
- `citros config show` - Display config
- `citros skill install/remove/list` - Skill management
- `citros audit show/verify` - Audit log access
- `citros sim status/reset` - Simulator control (pre-PoC)

**Dependencies**: `ct-core`, `clap`, `tokio`, `tracing`

---

## Data Flow

```
User Input (voice/text)
   ↓
ct-agent (orchestrator)
   ↓
ct-llm (intent classification - local or cloud)
   ↓
ct-agent (action planning)
   ↓
ct-security (policy evaluation)
   ↓
ct-agent (execution)
   ↓
ct-phone / ct-phone-sim (PhoneActions trait)
   ↓
Action Result → ct-agent → AgentResponse → User
```

All stages publish events to `ct-core::EventBus` for monitoring, logging, and coordination.

## Phase 0.5 vs Horizon 1 vs Horizon 2

| Crate | Phase 0.5 (Mac Mini) | Horizon 1 (Android) | Horizon 2 (OS) |
|-------|---------------------|---------------------|----------------|
| ct-core | ✓ Full implementation | ✓ Unchanged | ✓ Unchanged |
| ct-agent | ✓ Full implementation | ✓ Unchanged | ✓ Unchanged |
| ct-llm | ✓ Full implementation | ✓ Unchanged | ✓ Unchanged |
| ct-phone | ✗ Not used | ✓ Android-specific | ✗ Replaced by OS APIs |
| ct-phone-sim | ✓ Used for testing | ✗ Not needed | ✗ Not needed |
| ct-voice | ✗ Text-only | ✓ Voice I/O | ✓ Minor API updates |
| ct-security | ✓ Full implementation | ✓ + keystore integration | ✓ + HSM integration |
| ct-skills | ✓ Full implementation | ✓ Unchanged | ✓ Unchanged |
| ct-sync | ✓ Full implementation | ✓ Unchanged | ✓ Unchanged |
| ct-storage | ✓ Full implementation | ✓ Unchanged | ✓ Unchanged |
| ct-sensors | ✗ Minimal | ✓ Full Android sensors | ✓ OS-native sensors |
| ct-cli | ✓ Full implementation | ✓ Unchanged | ✓ Unchanged |

**Key Insight**: 85% of the codebase written in Phase 0.5 carries forward to Horizon 1 unchanged. 85% of Horizon 1 carries forward to Horizon 2. Only the phone abstraction layer (`ct-phone`) is disposable.

## Design Principles

1. **The `PhoneActions` trait is the abstraction boundary.** Both `SimulatedPhone` and `AndroidPhone` implement it. The agent doesn't know which it's talking to.

2. **100% reuse crates are architecture-neutral.** They work on macOS, Android, and the future OS without modification.

3. **Security as architecture.** The policy engine (`ct-security`) is a hard boundary that the agent cannot bypass.

4. **Local-first intelligence.** Simple tasks run entirely on-device. Cloud is for complex reasoning and backup only.

5. **Outbound-only networking.** Zero inbound ports, ever. The phone initiates all connections.

## Testing Strategy

- **Unit tests**: Each crate has tests for its public APIs
- **Integration tests**: End-to-end flows in `tests/` directory
- **TDD required**: All new features must have tests written first (RED → GREEN → REFACTOR)

## References

- **[SPEC.md](docs/SPEC.md)**: Complete technical specification and architecture decisions
- **[PRE-POC-PRD.md](docs/PRE-POC-PRD.md)**: Phase 0.5 sprint plan and task breakdown
- **[CLAUDE.md](CLAUDE.md)**: Code style, PR review process, and development guidelines

---

*Last updated: 2026-02-08 — Phase 0.5, Sprint 1, Epic 1*
