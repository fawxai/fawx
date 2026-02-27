# Fawx Roadmap

**Status**: Active  
**Updated**: 2026-02-27  

---

## Current State

The Fawx engine is built and running. 9 crates, 726 tests passing, TUI operational.

### Engine Crates

| Crate | Lines | Purpose |
|-------|-------|---------|
| fx-core | 642 | Core types, traits, errors |
| fx-kernel | 7,007 | Agentic loop, budget, context, auth, OAuth, policy, permissions |
| fx-llm | 5,679 | Provider clients (Anthropic, OpenAI, OpenAI Responses), model catalog, router, SSE |
| fx-cli | 5,449 | TUI shell, auth wizard, commands, eval harness (binary: `fawx`) |
| fx-agent | 4,697 | Intent classifier, plan builder, retry, skill tools, Claude client |
| fx-security | 3,834 | Policy engine, crypto, audit, rate limiting, signing |
| fx-skills | 2,719 | WASM skill runtime, manifest, registry, cache, loader |
| fx-storage | 2,258 | Encrypted storage, key derivation, credentials, conversation store |
| fx-loadable | 55 | Stub for hot-loadable modules (A/B slots, config, skills, strategies) |

**Additional:** `llama-cpp-sys` — local LLM bindings.

Binary: `fawx`. Config dir: `.fawx/`. Auth: `.fawx/auth.json`.

### Authentication

Three first-class auth methods:
1. **Claude subscription** (setup-token) — Claude Max users use existing subscription
2. **ChatGPT subscription** (PKCE OAuth) — ChatGPT Plus users use existing subscription  
3. **BYO API key** — Anthropic or OpenAI API keys

Subscription auth is not a hack — it's a first-class path. Most users already pay for an AI subscription; Fawx uses it.

---

## What's Done

### Phase 1: Engine Foundation (PRs 1–9) ✅

The original restructure plan had 12 PRs to transform from Android monolith to engine + shell. PRs 1–9 are merged:

- PR 1: Repo hygiene, stale files removed
- PR 2: Engine directory structure created
- PR 3: Core types extracted to fx-core
- PR 4: Provider clients (fx-llm) 
- PR 5: Budget system
- PR 6: Context management
- PR 7: Agent loop (fx-agent)
- PR 8: Prompt system
- PR 9: Security audit + policy engine hardening

### Rename: Citros → Fawx ✅

- All crate prefixes: ct-* → fx-*
- Binary: citros → fawx  
- Config dir: .citros/ → .fawx/
- Repo: abbudjoe/citros → abbudjoe/fawx

### TUI-First Pivot ✅

In late February 2026, the project pivoted from Android-first to TUI-first:
- The terminal interface validates the agentic loop before any GUI
- fx-cli is the first "shell" — a full TUI with auth wizard, conversation management, and eval harness
- Android/iOS/desktop are future shells that render UISpec
- PRs 10-12 (originally Kotlin decomposition + UniFFI bridge) are now about engine capabilities

---

## Revised Roadmap

### PR 10: Tool Execution + Response Quality 🔄

The engine can reason and respond. Next: execute tool calls and improve response quality.

- Tool execution framework in fx-agent
- File system tools (read, write, search)
- Shell execution (sandboxed)
- Self-bootstrapping: Fawx reads its own code, writes changes, runs tests
- Response quality improvements (context management, prompt tuning)

**Why this matters:** Once tool execution works, Fawx implements its own features. "Fawx builds Fawx." Joe plans to stream this — building in public.

### PR 11: Memory Persistence 📋

Currently conversations are ephemeral. This PR makes memory durable.

- Conversation persistence (fx-storage)
- Episodic memory: what happened, when, outcomes
- Semantic memory: durable facts, preferences
- Memory retrieval during perception step
- Foundation for memory consolidation ("dreaming")

### PR 12: UISpec System 📋

The contract between engine and shells. The engine generates UI specifications; shells render them.

- UISpec type definitions in fx-core
- Engine generates UISpec in response to user interactions
- TUI renders UISpec as terminal widgets
- This is what makes future shells (Android, iOS, desktop) possible

### Future: Additional Shells 🔮

Once UISpec exists, new shells are straightforward:
- **Desktop (Tauri/native)** — leverages local GPU for inference via llama-cpp-sys
- **Android** — Kotlin shell renders UISpec, uses existing android/ code as starting point
- **iOS** — Swift shell, same UISpec contract
- **Web** — browser-based shell

### Future: Ember Protocol Layer 🔮

Ember (the protocol layer, currently in the krust repo) becomes an independent dependency:
- Stateful protocol management
- Agent-to-agent communication
- Tool protocol standardization

---

## What Changed from the Original Plan

| Original (12-PR Android Migration) | Current (TUI-First) |
|---|---|
| PRs 10-12: Kotlin decomposition, UniFFI bridge, android/ restructure | PRs 10-12: Tool execution, memory persistence, UISpec |
| Android shell is THE target | TUI is the first shell; Android is ONE future shell |
| UniFFI bridge required for day-one functionality | UniFFI deferred — not needed until Android shell ships |
| "Kernel becomes the phone OS" | Desktop-first with GPU; phone is one shell |
| API key required | Subscription auth from day one |

---

## Success Criteria (Updated)

- [x] Engine contains all business logic, buildable and testable standalone
- [x] 726+ tests passing
- [x] TUI shell operational with auth wizard
- [x] Three auth methods working (Claude sub, ChatGPT sub, BYO key)
- [ ] Tool execution enables self-bootstrapping
- [ ] Memory persists across sessions
- [ ] UISpec contract defined and rendered by TUI
- [ ] At least one additional shell renders UISpec
