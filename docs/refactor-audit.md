# Refactor Audit — Doctrine Violations

**Tracking issue:** [#1638](https://github.com/abbudjoe/fawx/issues/1638)
**Doctrine:** [docs/doctrine.md](doctrine.md)
**Started:** 2026-03-26
**Status:** In progress

---

## Root Cause 1: Built-in tools aren't trait objects

The `Skill` trait correctly models WASM skills as self-describing, polymorphic trait objects. Built-in tools are methods on a 5,464-line monolith (`FawxToolExecutor`) with string-based dispatch. Every system that needs to classify a tool call reinvents its own string registry. These registries drift out of sync.

| ID | Category | Instance | Location | Severity | Notes |
|----|----------|----------|----------|----------|-------|
| RC1-01 | External behavior | Tool dispatch (25+ arm match) | `fx-tools/src/tools.rs:292` | Critical | Monolithic match dispatches all built-in tools |
| RC1-02 | External behavior | `tool_to_action_category` duplicated 3x | `fx-ripcord/evaluator.rs`, `fx-kernel/permission_gate.rs:356`, `fx-tools/tools.rs:264` | Critical | Three copies, all out of sync. `run_command` missing from 2 of 3 |
| RC1-03 | External behavior | `extract_journal_action` | `fx-ripcord/evaluator.rs` | Critical | Static match on tool names for journal entries. Proven broken (`run_command` never matched) |
| RC1-04 | Incomplete abstraction | `Skill` trait exists, built-in tools don't use it | `fx-loadable/skill.rs` vs `fx-tools/tools.rs` | Critical | The correct pattern exists; it was never applied to built-in tools |
| RC1-05 | Hardcoded discovery | `WRITE_TOOLS` static list | `fx-kernel/proposal_gate.rs:23` | High | Missing `run_command`, `memory_write`, `memory_delete`, `exec_background` |
| RC1-06 | Hardcoded discovery | `classify_shell_blind` missing `run_command` | `fx-kernel/proposal_gate.rs:374` | High | Kernel-blind protection broken for Fawx's actual shell tool |
| RC1-07 | Hardcoded discovery | `cacheability_for` static match | `fx-tools/tools.rs:264` | High | Every tool's cache policy in one match block |
| RC1-08 | Hardcoded discovery | `extract_index_paths` cache key match | `fx-kernel/caching_executor.rs:537` | High | Match on tool name to determine cacheable data paths |

**Fix:** `Tool` trait refactor. Built-in tools implement a trait with `name()`, `definition()`, `execute()`, `journal_hint()`, `cacheability()`, `side_effect_category()`, `cache_keys()`. The executor becomes a registry of trait objects. All 8 instances resolve.

---

## Root Cause 2: Provider metadata isn't on the provider trait

`LlmProvider` trait handles execution correctly. But catalog endpoints, thinking levels, auth header format, and fallback model lists live in external functions matching on provider name strings.

| ID | Category | Instance | Location | Severity | Notes |
|----|----------|----------|----------|----------|-------|
| RC2-01 | External behavior | `build_request` auth header dispatch | `fx-llm/model_catalog.rs:166` | Moderate | Match on provider name for auth header format |
| RC2-02 | External behavior | `hardcoded_fallback` model lists | `fx-llm/model_catalog.rs:315` | Moderate | Provider-specific model lists in a match block |
| RC2-03 | External behavior | `models_endpoint` URL match | `fx-llm/model_catalog.rs:384` | Moderate | Endpoint URLs matched by provider name |
| RC2-04 | External behavior | `supported_thinking_levels` | `fx-llm/lib.rs:120` | Moderate | Provider capabilities in an external function |
| RC2-05 | Incomplete abstraction | `LlmProvider` trait handles execution but not metadata | `fx-llm/provider.rs`, `fx-llm/lib.rs` | Moderate | The trait exists but is incomplete |
| RC2-06 | Hardcoded discovery | `PROVIDERS` array in doctor command | `fx-cli/commands/doctor.rs:16` | Low | Hardcoded provider list with URLs |

| RC2-07 | External behavior | `base_url_for_provider` match on provider name | `fx-cli/startup.rs:1921` | Moderate | Default API URLs matched by provider string |
| RC2-08 | External behavior | `models_for_provider` match on provider name | `fx-cli/startup.rs:1944` | Moderate | Default model lists matched by provider string |
| RC2-09 | External behavior | Provider registration if/else on `"anthropic"` | `fx-cli/startup.rs:1841` | Moderate | Constructs different provider types based on string |

**Fix:** Add metadata methods to `LlmProvider`: `catalog_endpoint()`, `thinking_levels()`, `fallback_models()`, `auth_header()`, `base_url()`, `default_models()`. Each provider declares its own capabilities. The catalog, doctor command, and startup discover providers through the trait. Provider registration becomes polymorphic via a factory trait or registration protocol.

---

## Root Cause 3: Monoliths resist composition

Large files that absorbed their children instead of composing them. Not string-matching violations, but they violate the fractal principle: same shape at every scale.

| ID | Category | Instance | Location | Severity | Notes |
|----|----------|----------|----------|----------|-------|
| RC3-01 | God object | `loop_engine.rs` (19,408 lines, 284 prod fns) | `fx-kernel` | High | Iteration, streaming, compaction, decomposition, synthesis, cancellation, budgets, scratchpads all in one file |
| RC3-02 | God object | `headless.rs` (5,373 lines) | `fx-cli` | Moderate | CLI routing, auth, config, analysis, improvement, keys, setup, signing |
| RC3-03 | God object | `config/lib.rs` (2,403 lines) | `fx-config` | Moderate | Config struct, validation, presets, serialization, env parsing |

**Fix:** Decompose into composable units. Loop engine: phase traits (`Think`, `Act`, `Observe`, `Compact`). Headless: command pattern. Config: separate preset, validation, and serialization modules.

---

## Root Cause 4: Stringly-typed dispatch in agent subsystem

Action steps use string `action` fields (`"tap"`, `"swipe"`, `"launch_app"`) matched in plan_builder.rs. Same anti-pattern as tool dispatch.

| ID | Category | Instance | Location | Severity | Notes |
|----|----------|----------|----------|----------|-------|
| RC4-01 | External behavior | `ActionStep.action` string dispatch | `fx-core/types.rs:152`, `fx-agent/plan_builder.rs:119` | Moderate | Action types are strings matched externally instead of typed trait objects |
| RC4-02 | Stringly-typed data | `ActionStep.parameters: HashMap<String, String>` | `fx-core/types.rs:156` | Moderate | Untyped parameter bags; each action knows its own schema but the type doesn't reflect it |
| RC4-03 | Stringly-typed data | `ActionResult.data: Option<HashMap<String, String>>` | `fx-core/types.rs:175` | Low | Untyped result data |

**Fix:** `Action` trait. Each action type (tap, swipe, launch, etc.) is a struct implementing a trait with `name()`, `execute()`, typed parameters. The plan builder works with trait objects. This will matter more as perception adds new action types.

---

## Root Cause 5: Feature flags as composition mechanism

Feature flags (`#[cfg(feature = "improvement")]`) gate tool inclusion in the `FawxToolExecutor` monolith. 10 instances in `tools.rs` alone. This is conditional compilation doing the job of runtime composition.

| ID | Category | Instance | Location | Severity | Notes |
|----|----------|----------|----------|----------|-------|
| RC5-01 | Feature-flag composition | `improvement` tools gated by `cfg(feature)` | `fx-tools/tools.rs` (10 instances) | Moderate | Tools conditionally compiled into the match block. With a trait registry, tools register at startup; no feature flags needed in dispatch. |
| RC5-02 | Feature-flag composition | `kernel-blind` gated in proposal_gate | `fx-kernel/proposal_gate.rs` | Low | Security feature gated by compile flag rather than runtime policy |

**Fix:** Resolves naturally with the `Tool` trait refactor (RC1). Tools register themselves at startup. Optional tools are included/excluded by registration, not compilation. Feature flags remain valid for entire crate inclusion/exclusion, not for match arm gating within a monolith.

---

## Confirmed Not Violations

| Instance | Location | Reason |
|----------|----------|--------|
| SSE event parsing | `fx-llm/anthropic.rs:544`, `openai_responses.rs:421` | Protocol boundary; match mirrors wire format. Not growing with the system. |
| Intent category parser | `fx-agent/intent/parser.rs:146` | LLM output parser; legitimately finite enum deserialization |
| `Surface` enum match | `fx-kernel/system_prompt.rs:269` | Rust enum IS the abstraction; exhaustive match is correct |
| `SessionStatus`/`MessageRole` matches | `fx-session/types.rs` | Finite, stable enums with `Display` impls |
| `PermissionPreset::from_str` | `fx-config/lib.rs:248` | Enum deserialization, not external classification |

---

## Unchecked Areas

- [x] `if/else` chains performing string-based dispatch → found RC2-07/08/09, RC4-01
- [ ] Cross-crate coupling (importing implementation details instead of going through traits) — light coupling found (fx-api → fx-tools for `ConfigSetRequest`), not systemic
- [x] Stringly-typed event/message passing → `StreamEvent` is properly typed enum ✅; `ActionStep` is stringly-typed → RC4
- [x] Builder patterns encoding knowledge → `LoopEngineBuilder` has 17 fields (symptom of RC3-01 god object, not independent violation)
- [x] Feature flags gating behavior that should be polymorphic → RC5

---

## Totals

| Root Cause | Instances | Severity Range |
|-----------|-----------|----------------|
| RC1: Built-in tools aren't trait objects | 8 | Critical-High |
| RC2: Provider metadata outside provider trait | 9 | Moderate-Low |
| RC3: Monoliths resist composition | 3 | High-Moderate |
| RC4: Stringly-typed agent actions | 3 | Moderate-Low |
| RC5: Feature flags as composition | 2 | Moderate-Low |
| **Total** | **25** | |

## Priority

1. **RC1** — biggest blast radius (8 instances), enables extensibility, blocks perception work. Also resolves RC5.
2. **RC3** — monoliths resist the composition perception needs. The 19K-line loop engine is the structural bottleneck.
3. **RC2** — 9 instances but lower urgency; provider set grows slowly.
4. **RC4** — will matter when perception adds new action types. Low urgency today.
5. **RC5** — resolves automatically with RC1.
