# Citros: AI-Native Phone Agent

[![Build Status](https://github.com/abbudjoe/citros/actions/workflows/ci.yml/badge.svg)](https://github.com/abbudjoe/citros/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.83%2B-orange.svg)](https://www.rust-lang.org/)

**Citros** is an AI-native phone agent designed to run locally on Android devices. Unlike cloud-based assistants, Citros lives on your phone, escalating to cloud LLMs for complex reasoning. It's the foundation for a future where the agent *is* the interface, not just a feature bolted onto an app-centric OS.

**⚠️ Status: Early Development** — Citros is under active development. Core infrastructure (Epics 1-6) is complete, with 225+ tests and a clean Rust codebase. See [Project Status](#project-status) below for details.

---

## Table of Contents

- [Vision](#vision)
- [Project Status](#project-status)
- [Architecture](#architecture)
- [Features](#features)
- [Getting Started](#getting-started)
- [Testing](#testing)
- [Security](#security)
- [Contributing](#contributing)
- [Roadmap](#roadmap)
- [License](#license)

---

## Vision

### The Problem
Smartphones haven't fundamentally changed since 2007. Users navigate grids of siloed apps, manually orchestrating multi-step tasks across different UIs. AI assistants (Siri, Google Assistant) failed to fix this because they're constrained by OS limitations and predefined app "intents."

### The Solution
**Citros puts the agent on the phone, not in the cloud.** It perceives (voice, screen, sensors), thinks (local LLM for speed, cloud for complexity), and acts (direct device control). The phone becomes an *awareness surface* where you glance to see what the agent is doing, not an *input surface* where you tap 47 times to book a flight.

### Design Principles
1. **Phone-native, not server-first** — No gateway, no open ports, no inbound connections
2. **Outbound-only networking** — Zero attack surface from inbound connections
3. **Local-first intelligence** — Simple tasks run on-device, cloud only for complex reasoning
4. **Security as architecture** — Hardware-backed encryption, policy engine, append-only audit log
5. **PoC code is OS code** — Everything we build for Android will carry forward to the eventual OS

For the full vision and three-horizon roadmap, see [`docs/SPEC.md`](docs/SPEC.md).

---

## Project Status

### Completed (Epics 1-6)
✅ **Foundation** — Rust workspace with 12 crates, clean dependency structure  
✅ **LLM Integration** — llama.cpp bindings for local inference, model loading, GGUF support  
✅ **Claude API Client** — Async HTTP client, streaming responses, error handling  
✅ **Policy Engine** — TOML-based policies, capability matching, action validation  
✅ **Encrypted Storage** — ChaCha20-Poly1305 encryption, key management, secure persistence  
✅ **Intent Classification** — LLM-based intent parsing, confidence thresholding, entity extraction  

**Total:** 225+ tests across all crates, passing `cargo clippy` with zero warnings.

### In Progress (Epics 7-9)
🚧 **WASM Skill Runtime** — Load/execute WASM modules, host API versioning  
🚧 **Audit Log** — Tamper-proof action logging, CLI query interface, HMAC verification  
🚧 **Interactive CLI** — Conversational interface, command routing, LLM integration  

### Not Yet Started
📋 **Phone Actions** (Epic 4) — Android UI automation, touch injection, app launching  
📋 **Voice Interface** (Epic 7) — Speech-to-text, text-to-speech, wake word detection  
📋 **Multi-Device Sync** (Epic 10) — Encrypted sync protocol, conflict resolution  
📋 **Observability** (Epic 11) — Metrics, tracing, performance monitoring  

---

## Architecture

Citros is a Rust workspace with 12 crates, designed for modularity and testability:

```
citros/
├── crates/
│   ├── ct-core/           # Core types, traits, and utilities
│   ├── ct-agent/          # Agent orchestration, intent classification, Claude API
│   ├── ct-llm/            # Local LLM inference via llama.cpp
│   ├── ct-security/       # Policy engine, capability management, action validation
│   ├── ct-storage/        # Encrypted key-value store, audit log persistence
│   ├── ct-skills/         # WASM skill runtime, host API, module loading
│   ├── ct-phone/          # Phone-specific actions (UI automation, app control)
│   ├── ct-voice/          # Voice interface (STT, TTS, wake word)
│   ├── ct-sensors/        # Sensor access (camera, microphone, location, etc.)
│   ├── ct-sync/           # Multi-device encrypted sync protocol
│   ├── ct-cli/            # Interactive CLI for development and testing
│   └── ct-audit/          # Audit log (CLI commands, verification, querying)
└── ffi/
    └── llama-cpp-sys/     # Rust FFI bindings to llama.cpp

```

### Key Dependencies
- **llama.cpp** — Local LLM inference (GGUF models, CPU/GPU acceleration)
- **Wasmtime** — WASM runtime for skills/services
- **Tokio** — Async runtime for non-blocking I/O
- **reqwest** — HTTP client for Claude API (outbound-only, rustls-tls)
- **redb** — Embedded KV store for encrypted persistence
- **ring** — Cryptography (ChaCha20-Poly1305, HKDF, HMAC-SHA256)

### Data Flow
```
User Input (Voice/CLI)
    ↓
Intent Classifier (LLM)
    ↓
Agent Orchestrator
    ↓
Policy Engine ← Validates → Action
    ↓
Encrypted Storage / Audit Log
    ↓
Phone Actions / WASM Skills
    ↓
Device (Android UI, Apps, Sensors)
```

---

## Features

### Implemented
- **Intent Classification** — Parses user input into 9 categories (LaunchApp, Search, Message, Calendar, etc.) with confidence scoring and entity extraction
- **Intent Metrics** — Tracks total classifications, average confidence, latency, and fallback rate
- **Policy Engine** — TOML-based action validation, capability matching, async policy loading
- **Encrypted Storage** — ChaCha20-Poly1305 encryption, secure key generation, atomic writes
- **Audit Log** — Tamper-proof action logging with HMAC-SHA256 verification
- **Local LLM Inference** — llama.cpp integration, model loading, streaming responses
- **Claude API Client** — Async HTTP client, streaming, retry logic, error handling
- **WASM Skill Runtime** — Load and execute WASM modules with host API versioning

### Security Features
- **Hardware-backed encryption** — ChaCha20-Poly1305 with HKDF key derivation
- **Policy-based access control** — Every action validated against TOML policy files
- **Append-only audit log** — Tamper-proof logging with HMAC verification
- **Outbound-only networking** — Zero inbound connections, no open ports
- **Secure key management** — Keys stored with 0600 permissions, never logged
- **WASM sandboxing** — Skills run in isolated WASM environment with restricted host API

### Planned Features
- **Voice control** — Wake word detection, on-device STT, natural TTS
- **Android UI automation** — Touch injection, app launching, screen reading
- **Multi-device sync** — End-to-end encrypted sync across phones
- **Proactive agent** — Background task execution, notification handling
- **Skill ecosystem** — Installable WASM skills for custom capabilities

---

## Getting Started

### Prerequisites
- **Rust 1.83+** — Install from [rustup.rs](https://rustup.rs/)
- **Git** — For cloning the repository
- **C++ compiler** — For building llama.cpp (gcc or clang)

### Clone and Build
```bash
# Clone the repository
git clone https://github.com/abbudjoe/citros.git
cd citros

# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Build in release mode (optimized)
cargo build --workspace --release
```

### Quick Test: Intent Classification
```bash
# Run the intent classification tests
cargo test -p ct-agent --test tests -- intent

# Run with output to see classification results
cargo test -p ct-agent --test tests -- intent --nocapture
```

### Environment Setup
To use cloud LLM features (Claude API), set your API key:
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

Subscription-first auth routing can also use:
```bash
# Claude Code subscription token from `claude setup-token`
# Accepts any of: CLAUDE_SETUP_TOKEN, CLAUDE_CODE_SETUP_TOKEN, or ANTHROPIC_SUBSCRIPTION_TOKEN
export CLAUDE_SETUP_TOKEN="..."

# Optional OpenAI subscription OAuth token fallback
export OPENAI_OAUTH_TOKEN="..."

# Optional Claude CLI stream bridge URL (experimental)
export CLAUDE_SDK_URL="ws://127.0.0.1:4242"
```

### Android OAuth Setup

To enable browser-based OpenAI login in the Android app:

1. Start an OAuth bridge service (see `docs/codex-oauth-bridge-api.md` for implementation spec)
2. In Citros Android app, select **"🧪 Codex OAuth (Browser Redirect)"**
3. Enter bridge URL (default: `http://127.0.0.1:4318`)
4. Complete sign-in in browser, app will auto-detect the redirect and exchange the code

**API contract:** `docs/codex-oauth-bridge-api.md`

To run a local OAuth bridge server for Android Codex sign-in:
```bash
export CITROS_OPENAI_AUTH_URL="https://<provider-authorize-endpoint>"
export CITROS_OPENAI_TOKEN_URL="https://<provider-token-endpoint>"
export CITROS_OPENAI_CLIENT_ID="<oauth-client-id>"
# Optional for confidential clients:
export CITROS_OPENAI_CLIENT_SECRET="<oauth-client-secret>"
export CITROS_OPENAI_SCOPE="openid profile email offline_access"

# Start bridge (default listen: 127.0.0.1:4318)
cargo run -p ct-cli -- oauth-bridge
```

For Android emulator use `http://10.0.2.2:4318`. For physical devices use:
```bash
adb reverse tcp:4318 tcp:4318
```

### Android Developer Bring-up Docs

- NDK cross-compilation baseline: [`docs/android-setup.md`](docs/android-setup.md)
- **NDK cross-compilation + hello-world on rooted Pixel 10 (arm64)** — [`docs/android-ndk-cross-compilation.md`](docs/android-ndk-cross-compilation.md)
- Root/Magisk setup (dedicated dev devices): [`docs/android-root-magisk-setup.md`](docs/android-root-magisk-setup.md)
- ADB workflow (build/install/push/debug): [`docs/android-adb-workflow.md`](docs/android-adb-workflow.md)

---

## Testing

Citros has extensive test coverage across all crates:

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p ct-agent
cargo test -p ct-security
cargo test -p ct-storage

# Run with coverage report (requires cargo-tarpaulin)
cargo tarpaulin --workspace --out Html

# Run clippy (zero warnings policy)
cargo clippy --workspace --all-targets -- -D warnings

# Check formatting
cargo fmt --check
```

### Test Organization
- **Unit tests** — In each module, testing individual functions and structs
- **Integration tests** — In `tests/` directories, testing cross-crate interactions
- **Property tests** — Using QuickCheck for randomized testing (planned)
- **End-to-end tests** — Simulating full agent workflows (planned)

---

## Security

Citros takes security seriously. The agent runs with elevated privileges on the phone, so the security model must be proportionally rigorous.

### Threat Model
1. **Malicious WASM skills** — Sandboxed with restricted host API, policy validation
2. **LLM prompt injection** — Intent parsing validation, entity sanitization
3. **Local data theft** — All storage encrypted at rest, keys in secure keystore
4. **Network eavesdropping** — TLS 1.3 for all outbound connections
5. **Tampering with audit log** — HMAC-SHA256 verification, append-only structure

### Security Features
- **Encrypted storage** — ChaCha20-Poly1305 with hardware-backed keys
- **Policy engine** — Every action must pass policy validation before execution
- **Audit log** — Tamper-proof logging with hash chain verification
- **WASM sandboxing** — Skills isolated from system, limited host API access
- **Outbound-only** — Zero inbound ports, all connections initiated by phone

### Reporting Security Issues
Please **do not** open public GitHub issues for security vulnerabilities. Contact the maintainers directly.

---

## Contributing

Citros is in early development and not yet accepting external contributions. Once the core architecture stabilizes (post-Epic 9), we'll publish contribution guidelines and open the project to community involvement.

### For Now
- **Star the repo** if you're interested in the project
- **Watch releases** to get notified of major milestones
- **Read the spec** ([`docs/SPEC.md`](docs/SPEC.md)) to understand the vision
- **Join discussions** in GitHub Discussions (coming soon)

### Development Setup
If you're exploring the codebase:

```bash
# Install development tools
rustup component add rustfmt clippy

# Set up pre-commit hooks (optional)
cargo install cargo-husky

# Run the full CI check locally
cargo fmt --check && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace

# Run Android sensor timeout/concurrency CI-targeted suites
# Subshell keeps the repo root shell unchanged while running Gradle from android/.
(cd android && ./gradlew :core:phoneAgentApiSensorCiTest :chat:androidSensorProviderCiTest)
```

---

## Roadmap

### Horizon 1: Android Proof of Concept (Months 1-12)
**Goal:** Rust daemon on rooted Pixel 10 Pro that proves the value of a phone-native agent.

- [x] **Epic 1:** Project foundation (12-crate Rust workspace)
- [x] **Epic 2:** LLM integration (llama.cpp, intent classification)
- [x] **Epic 3:** Claude API client (async HTTP, streaming)
- [x] **Epic 5:** Policy engine (TOML policies, capability validation)
- [x] **Epic 6:** Encrypted storage (ChaCha20-Poly1305, secure keys)
- [ ] **Epic 4:** Phone actions (Android UI automation, app control)
- [ ] **Epic 7:** Voice interface (STT, TTS, wake word detection)
- [ ] **Epic 8:** WASM skill runtime (sandboxed skills, host API)
- [ ] **Epic 9:** Interactive CLI (conversational interface, debugging)
- [ ] **Epic 10:** Multi-device sync (encrypted sync protocol)
- [ ] **Epic 11:** Observability (metrics, tracing, logging)

### Horizon 2: AI-Native Operating System (Months 12-28)
Replace Android userspace with agent-native runtime. Custom compositor, telephony via oFono, WASM services replace apps. 85% of PoC code carries forward.

### Horizon 3: Purpose-Built Hardware (Months 28-48)
ODM partnership or custom board design. Hardware trust button, NPU-optimized SoC, refined form factor.

---

## Project Structure

```
citros/
├── crates/              # Rust workspace crates
├── docs/                # Design docs, specs, architecture diagrams
├── ffi/                 # FFI bindings (llama.cpp)
├── scripts/             # Build scripts, dev tools
├── .github/             # GitHub Actions CI workflows
├── Cargo.toml           # Workspace manifest
├── Cargo.lock           # Dependency lockfile
└── README.md            # This file
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

## Acknowledgments

- **OpenClaw** — Inspiration for skill ecosystem and agent-first design
- **llama.cpp** — Enabling local LLM inference on constrained devices
- **Anthropic** — Claude API powering complex reasoning tasks
- **Rust community** — For the tools that make safe systems programming possible

---

**Built with 🦀 Rust** | **Designed for privacy, speed, and autonomy**
