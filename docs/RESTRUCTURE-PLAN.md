# Citros Restructure Plan

**Status**: Draft — ready for review  
**Date**: 2026-02-26  
**Goal**: Transform the repo from Android-only to multi-platform (Rust engine + Kotlin/Swift shells), preparing for the Rust OS endgame.

---

## Current State

### What exists

```
citros/
├── android/                    ← Kotlin Android app
│   ├── core/ (122 .kt, 23k lines)   ← ALL business logic lives here
│   ├── chat/ (62 .kt)               ← UI, services, overlays
│   └── preview/                      ← Compose previews
├── crates/                     ← Rust workspace (partially implemented)
│   ├── ct-agent/    (4.7k lines, 78 tests)   ← Agent loop, executor
│   ├── ct-cli/      (2.9k lines, 38 tests)   ← CLI binary + eval harness
│   ├── ct-core/     (642 lines, 1 test)       ← Core types
│   ├── ct-llm/      (1.9k lines, 28 tests)    ← LLM client abstractions
│   ├── ct-security/ (3.8k lines, 83 tests)    ← Policy engine, crypto
│   ├── ct-skills/   (2.7k lines, 65 tests)    ← WASM skill runtime
│   ├── ct-storage/  (2.2k lines, 60 tests)    ← Encrypted storage
│   ├── ct-phone/    (54 lines, stub)
│   ├── ct-sensors/  (24 lines, stub)
│   ├── ct-sync/     (24 lines, stub)
│   └── ct-voice/    (20 lines, stub)
├── ffi/llama-cpp-sys/          ← llama.cpp bindings
├── docs/, scripts/, skills/    ← Docs, tooling, WASM skill examples
└── scratch/, tmp/              ← Junk
```

### The problem

1. **`android/core/` is a 23k-line monolith.** Everything is in one flat package: provider clients, agent executor, screen reader, phone tools, budget system, policy engine, prompt builder, recovery manager, context compaction, model catalog, task state, wallet, TTS/STT, web search — all in `ai.citros.core`.

2. **Kotlin and Rust are duplicating work.** The Rust crates implement agent logic, security, storage, and skills. The Kotlin `core/` implements much of the same. Neither is authoritative.

3. **No clear engine boundary.** There's no API contract between "things that need Android" and "things that are pure logic." Everything depends on everything.

---

## Target State

```
citros/
├── engine/                          ← Rust shared core (THE authority)
│   ├── Cargo.toml                   ← Workspace root
│   └── crates/
│       ├── ct-core/                 ← Types, traits, error types
│       ├── ct-agent/                ← Agent loop, executor, state machine
│       ├── ct-llm/                  ← Provider clients (OpenAI, Anthropic, OR)
│       ├── ct-security/             ← Policy engine, crypto, audit
│       ├── ct-skills/               ← WASM skill runtime
│       ├── ct-storage/              ← Encrypted local storage
│       ├── ct-context/              ← Context compaction, memory, trimming
│       ├── ct-budget/               ← Spending guards, cost estimation
│       ├── ct-eval/                 ← Determinism eval harness
│       └── ct-bridge/               ← UniFFI definitions → generates bindings
├── android/                         ← Kotlin UI shell (THIN)
│   ├── app/                         ← Main app module
│   │   └── src/main/kotlin/
│   │       ├── ui/                  ← Compose screens, themes, components
│   │       ├── service/             ← AccessibilityService, OverlayService
│   │       ├── bridge/              ← UniFFI-generated Kotlin bindings + adapters
│   │       └── platform/            ← Android-specific: sensors, TTS, screen reader
│   └── build.gradle.kts
├── ios/                             ← Swift UI shell (future)
├── docs/
│   ├── architecture/                ← ADRs, diagrams
│   ├── specs/                       ← Product specs
│   └── runbooks/                    ← Operational guides
├── scripts/
│   ├── build-android.sh             ← cargo build + copy .so
│   └── test.sh                      ← Run all tests (Rust + Kotlin)
└── .github/
    └── workflows/
        ├── rust.yml                 ← cargo fmt/test/clippy
        ├── android.yml              ← Gradle build + test
        └── integration.yml          ← Full stack (Rust build → Android test)
```

### What moves where

| Current location | Destination | Rationale |
|---|---|---|
| `android/core/AgentExecutor.kt` | `engine/crates/ct-agent/` | Pure execution logic, no Android deps |
| `android/core/PhoneAgentApi.kt` | Split: engine logic → `ct-agent/`, Android glue → `android/app/bridge/` | 2849-line god object, must be decomposed |
| `android/core/AnthropicClient.kt`, `OpenAiClient.kt`, `OpenRouterClient.kt`, `BaseProviderClient.kt` | `engine/crates/ct-llm/` | HTTP clients, no Android deps |
| `android/core/PhoneAgentPrompts.kt`, `AgentPromptBuilder.kt` | `engine/crates/ct-agent/` | Prompt construction is pure string logic |
| `android/core/BoundaryCheck.kt`, `FailureFallbackStateMachine.kt`, `RecoveryScaffold.kt` | `engine/crates/ct-agent/` | Loop control, state machines |
| `android/core/ActionPolicy*.kt`, `PolicyDecision.kt`, `PolicyAuditLogger.kt` | `engine/crates/ct-security/` | Policy engine |
| `android/core/BudgetGuard.kt`, `BudgetStore.kt`, `BudgetConfig.kt`, `CostEstimator.kt`, `TaskCostSummary.kt`, `TaskTokenAccumulator.kt` | `engine/crates/ct-budget/` | Spending logic, no Android deps |
| `android/core/ContextCompactor.kt`, `ContextManager.kt`, `TrimmingPolicy.kt`, `MemoryProvider.kt` | `engine/crates/ct-context/` | Context management |
| `android/core/WebSearchClient.kt`, `TinyFishClient.kt`, `WebFetchClient.kt` | `engine/crates/ct-llm/` (or new `ct-web/`) | HTTP clients |
| `android/core/Message.kt`, `ToolUse.kt`, `AgentState.kt`, `ModelConfig.kt`, `ModelCatalog.kt` | `engine/crates/ct-core/` | Core types |
| `android/core/ScreenReader.kt`, `ScreenContent.kt`, `PhoneTools.kt`, `PhoneActions.kt` | `android/app/platform/` | Android-specific (Accessibility APIs) |
| `android/core/*TextToSpeech*.kt`, `*SpeechToText*.kt`, `VoiceManager.kt` | `android/app/platform/` | Android audio APIs |
| `android/core/KeyStore.kt`, `WalletManager.kt`, `WalletKey.kt` | `engine/crates/ct-security/` (logic) + `android/app/platform/` (Android Keystore) |
| `android/core/OverlayState.kt`, `ActionPill.kt` | `android/app/ui/` | UI state |
| `android/chat/*` | `android/app/ui/` + `android/app/service/` | Chat UI, accessibility service, overlays |

### What gets deleted

| Item | Reason |
|---|---|
| `scratch/`, `tmp/` | Junk |
| `citros-ui-mocks.html` | Obsolete |
| `h1-pr5-tool-gating-pressure-test.md` | One-off test doc, archive or delete |
| `UI-REDESIGN-RECOMMENDATIONS.md` | Move to `docs/` or archive |
| `AGENTS.md`, `CLAUDE.md`, `CODEX.md` | AI agent config files, don't belong in repo root |
| Stub crates (`ct-phone`, `ct-sensors`, `ct-sync`, `ct-voice`) | 20-54 lines of nothing. Recreate when actually needed |

---

## PR Sequence (Incremental)

Each PR is self-contained. App works after every merge.

### PR 1 — Repo hygiene
- Delete `scratch/`, `tmp/`, stale root files
- Move `UI-REDESIGN-RECOMMENDATIONS.md` to `docs/archive/`
- Remove stub crates (ct-phone, ct-sensors, ct-sync, ct-voice)
- Move `AGENTS.md`, `CLAUDE.md`, `CODEX.md` out of repo (or `.gitignore`)
- Add `ENGINEERING.md` to repo root (already done on merge branch)
- **Tests**: existing tests still pass, no behavior change

### PR 2 — Create engine directory structure
- Move `crates/` → `engine/crates/`
- Move `Cargo.toml` → `engine/Cargo.toml`
- Move `ffi/` → `engine/ffi/`
- Update all `path = ` references in Cargo.toml files
- Add `engine/crates/ct-bridge/` stub with UniFFI setup
- Add `engine/crates/ct-budget/` stub
- Add `engine/crates/ct-context/` stub
- Add `scripts/build-android.sh` placeholder
- **Tests**: `cd engine && cargo test --workspace` passes

### PR 3 — Extract core types to ct-core
- Move `Message.kt`, `ToolUse.kt`, `AgentState.kt`, `ModelConfig.kt`, `ModelCatalog.kt` logic to Rust `ct-core/`
- Define equivalent types in Rust with serde
- Add UniFFI annotations to `ct-bridge/`
- Kotlin files remain but become thin wrappers calling Rust (or initially just coexist)
- **Tests**: Rust unit tests for all moved types. Kotlin tests unchanged.

### PR 4 — Extract provider clients to ct-llm
- Port `BaseProviderClient.kt`, `AnthropicClient.kt`, `OpenAiClient.kt`, `OpenRouterClient.kt` to Rust
- Rust versions use `reqwest`, match existing HTTP behavior
- Tests: TDD — write Rust tests first, then implement
- Kotlin clients remain temporarily, flagged for removal
- **Tests**: Rust client tests with mock HTTP. Kotlin tests unchanged.

### PR 5 — Extract budget system to ct-budget
- Port `BudgetGuard.kt`, `BudgetStore.kt`, `BudgetConfig.kt`, `CostEstimator.kt`, `TaskTokenAccumulator.kt`
- Pure logic, no Android deps
- **Tests**: Full Rust coverage for budget calculations, guard logic, store operations.

### PR 6 — Extract context management to ct-context
- Port `ContextCompactor.kt`, `ContextManager.kt`, `TrimmingPolicy.kt`
- Pure string/message manipulation
- **Tests**: Rust tests for compaction, trimming, message management.

### PR 7 — Extract agent loop to ct-agent
- Port `AgentExecutor.kt`, `BoundaryCheck.kt`, `FailureFallbackStateMachine.kt`, `RecoveryScaffold.kt`
- Wire to `ct-core` types, `ct-security` policy, `ct-budget` guards
- This is the big one — the execution loop moves to Rust
- **Tests**: Full state machine transition coverage, boundary check tests, recovery tests.

### PR 8 — Extract prompts to engine
- Port `PhoneAgentPrompts.kt`, `AgentPromptBuilder.kt` to Rust
- Pure string construction, no Android deps
- **Tests**: Prompt assembly tests, mode/tier coverage.

### PR 9 — UniFFI bridge + Android integration
- Complete `ct-bridge/` with UniFFI definitions for all exported types/functions
- `scripts/build-android.sh` builds `.so` for `aarch64-linux-android`
- Add `.so` to `android/app/src/main/jniLibs/`
- Kotlin bridge adapters in `android/app/bridge/`
- First end-to-end: Kotlin → JNI → Rust engine → response → Kotlin
- **Tests**: Integration test proving round-trip works.

### PR 10 — Slim PhoneAgentApi
- Decompose the 2849-line `PhoneAgentApi.kt`:
  - Engine logic now calls Rust via bridge
  - Android glue (screen reader, tool execution) stays Kotlin
  - PhoneAgentApi becomes an orchestrator that calls bridge + platform
- **Tests**: Existing PhoneAgentApi tests adapted, new integration tests.

### PR 11 — Restructure android/ directory
- Reorganize from flat `core/` + `chat/` to:
  - `app/ui/` — Compose screens, themes
  - `app/service/` — Accessibility, overlay, notifications
  - `app/bridge/` — UniFFI adapters
  - `app/platform/` — Android-specific (screen reader, TTS, sensors)
- **Tests**: All tests migrated to new paths, still passing.

### PR 12+ — Ongoing cleanup
- Remove Kotlin implementations that are now in Rust
- Remove dead code, unused imports, vestigial files
- Update docs to reflect new architecture

---

## Dependencies

```
PR 1 ──→ PR 2 ──→ PR 3 ──┬──→ PR 4
                          ├──→ PR 5
                          ├──→ PR 6
                          └──→ PR 7 ──→ PR 8 ──→ PR 9 ──→ PR 10 ──→ PR 11
```

PRs 4, 5, 6 can run in parallel after PR 3.
PR 7 (agent loop) depends on types from 3 and should ideally follow 4-6.
PR 9 (bridge) needs most engine crates in place.

---

## Risk Mitigation

| Risk | Mitigation |
|---|---|
| App breaks during migration | Each PR keeps app functional. Feature flag Rust engine (Kotlin fallback) |
| UniFFI doesn't support a pattern we need | Generated code is readable; can hand-edit or supplement |
| Rust cross-compilation issues | Build script tested early (PR 2). aarch64-linux-android target well-supported |
| Test coverage drops during migration | ENGINEERING.md rule: "if you touch it, you test it" |
| Migration takes too long | PRs 4-6 parallelizable. Each PR is independently valuable |

---

## Success Criteria

- [ ] `engine/` contains all business logic, buildable and testable without Android SDK
- [ ] `android/` is a thin UI shell (<5k lines of non-generated Kotlin)
- [ ] App functionality unchanged from user perspective
- [ ] All engine crates have >80% test coverage
- [ ] `cargo test --workspace` and `./gradlew testDebugUnitTest` both pass
- [ ] Build script produces working APK from clean checkout
