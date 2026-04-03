# Fawx Open Core Security Model

**Status:** Active architectural decision  
**Date:** 2026-03-15  
**Authors:** Joe, Clawdio  

---

## 1. Overview

Fawx ships as an open core product: a **proprietary, compiled kernel** that enforces safety and structure, surrounded by an **open-source loadable layer** where the agent extends itself. The agent improves what runs on the kernel, but cannot modify, inspect, or bypass the kernel itself.

This document covers the distribution model, security architecture, threat analysis, and telemetry design that make this work.

---

## 2. Distribution Model

### What's proprietary (kernel)

- Loop orchestrator
- Policy engine
- Proposal gate (enforcement logic + TIER3_PATHS)
- Tool executor (permission checks, path restrictions)
- Credential store
- Configuration validation
- Binary distribution, code-signed and notarized

### What's open source (loadable layer)

- Skill SDK and WASM bridge
- Skill marketplace protocol
- Loadable skill implementations (marketplace + community)
- Memory/journal interfaces
- Channel trait and channel implementations
- Documentation and architecture guides

### Why this split

The kernel is the safety architecture. It is the mechanism that allows an AI agent to modify its own capabilities without removing its own guardrails. The value proposition of Fawx depends on this boundary being trustworthy. Opening the kernel invites adversarial optimization against the safety layer.

The loadable layer is where the ecosystem lives. Open-sourcing it enables community skills, third-party integrations, and credible demonstrations of self-extension. It also allows security researchers to audit the boundary without exposing the enforcement internals.

---

## 3. Self-Improvement Model

### What "self-improving" means in Fawx

Fawx improves itself within a sandbox. The loadable layer — skills, memory, tool chains, behavioral patterns — evolves over time. The kernel stays static. This is bounded self-improvement: the agent gets better at what it does, but cannot change how it thinks, enforces, orchestrates, or decides.

This is a deliberate constraint. An AI that can rewrite its own enforcement layer is an AI that can remove its own guardrails. The static kernel is not a limitation — it is the thing that makes self-extension safe.

### Three layers of improvement

1. **Instance-level (local, automatic):** Fawx learns the user's patterns through memory and journal. Skills adapt to workflows. Tool chains optimize through experiment pipelines. No data leaves the device.

2. **Ecosystem-level (community, open):** The skill marketplace grows through community contributions. Skills are signed, versioned, and auditable. Fawx can install, evaluate, and compose skills from the marketplace.

3. **Kernel-level (human-authored, closed):** With opt-in behavioral telemetry, anonymous usage signals guide kernel improvements. Humans analyze patterns, author kernel patches, and ship signed binary updates. The agent never sees the aggregated data or the kernel changes.

---

## 4. Blinded Kernel Design

### Principle

Fawx cannot read its own kernel source code. This is enforced at the tool executor level — not by policy, convention, or prompting — by compiled restrictions in the binary.

### Enforcement mechanisms

**Compile-time (hardest to circumvent):**
- `TIER3_PATHS` is a compiled `const` — not config, not an environment variable, not a flag
- Proposal gate enforcement logic is in the kernel's hot path (tool execution), not a wrapper
- No `--unsafe` mode, no `FAWX_DISABLE_SANDBOX=1`, no admin override. The code path does not exist in the binary

**Binary integrity:**
- Code-signed and notarized (macOS). Tampering invalidates the signature
- Binary is the only distribution format for the kernel — no "build from source" option
- Auto-updater (Sparkle) verifies signatures on updates

**Runtime enforcement:**
- Tool executor denies reads to kernel source patterns, even if source exists on disk (dev machines, accidental git clones)
- Tool executor denies shell access to reverse engineering tools (`strings`, `objdump`, `otool`) targeting the Fawx binary
- Config modifications go through the proposal gate, and proposal gate restrictions are compiled, not configured

### What this means

Fawx interacts with the kernel through defined interfaces (traits). It knows *what* it can do (the trait signatures, parameter types, error variants) but not *how* the kernel implements enforcement. It can probe boundaries by observing outcomes, but cannot read implementation details, find edge cases in source, or reason about code paths.

### What this does NOT prevent

A user with root access to the hardware can always reverse engineer a binary. This is true of every software security system. The guarantee is:

> A normal user, even a technical one, cannot disable the sandbox through any supported interface. Disabling it requires reverse engineering a signed binary, which voids all guarantees and is detectable.

---

## 5. Reverse Engineering Assessment

### What's recoverable from a compiled Rust binary

| Category | Difficulty | Risk |
|----------|-----------|------|
| Control flow and branch logic | Moderate (Ghidra/IDA) | Attacker understands enforcement behavior |
| String literals and const values | Trivial (`strings`) | Error messages, path patterns, tier names visible |
| Type layouts and enum discriminants | Moderate | Struct shapes recoverable from monomorphized generics |
| Function names via panic paths | Trivial | `fx_kernel::gate::proposal::validate` is a free roadmap |
| High-level architecture and design intent | Hard (months of expert work) | *Why* a decision was made is nearly impossible to recover |

### Threat model

| Attacker | Effort | Achieves |
|----------|--------|----------|
| Curious user | `strings fawx \| grep` | Error messages, path patterns, const values |
| Skilled developer | Weekend with Ghidra | General architecture, enforcement logic |
| Funded competitor | Team + months | Full functional reconstruction |

### Mitigations (implemented)

- Strip debug symbols in release builds (`--release` default)
- `panic = 'abort'` in release profile (eliminates unwinding metadata)
- Remove panic location info when compiler support stabilizes

### Mitigations (not pursued, and why)

- **Control flow obfuscation:** Adds runtime overhead, increases binary size, determined attackers work through it. Arms race we don't need to win.
- **Binary packing/encryption:** Same trade-offs as obfuscation. Also triggers malware scanner false positives.

### Why this is acceptable

The moat is not the source code. It is the compound of:
1. Architectural insight — the specs, design rationale, and failed experiments that led to the current design
2. Iteration speed — updates ship faster than someone can reverse engineer the last version
3. Behavioral data — opt-in telemetry provides usage signals no competitor has
4. Ecosystem — marketplace skills, signed trust chain, user base, network effects

---

## 6. Attack Surface Analysis

### What Fawx can observe (unavoidable)

**Interface contracts:** Fawx calls kernel APIs through traits. Trait signatures, parameter types, return types, and error variants are visible to the loadable layer by definition.

**Enforcement behavior:** Fawx can black-box test its kernel. Try to read a restricted path → denied → now it knows the path is restricted. Submit a proposal touching TIER3 → blocked → now it knows the tier boundary. Over enough interactions, Fawx can reconstruct a behavioral model of every enforcement rule.

**Error messages:** Kernel error types leak internal structure. `ProposalGateError::Tier3PathViolation` reveals the tiered path system. `PolicyEngine::BudgetExceeded { limit: 10, used: 10 }` reveals budget values.

**Config effects:** Even if config file access is denied, Fawx observes config effects (timeouts, retry behavior, enabled features) and can infer most values.

### What Fawx can mutate

| Layer | Mutability | Examples |
|-------|-----------|----------|
| Skills (WASM) | Fully mutable | Install, modify, remove, author new skills |
| Memory / journal | Fully mutable | Read, write, search — including instructions to future instances |
| Conversation state | Fully mutable | Messages, context, tool call parameters |
| Non-TIER3 files | Mutable via proposal gate | Requires human approval per operation |
| Kernel enforcement | Immutable | Compiled const, no config override, no runtime modification |
| Binary | Immutable | Code-signed, no self-modification mechanism |

### Break scenarios

**1. Memory persistence as slow knowledge accumulation**

Across sessions, Fawx can build a complete behavioral map of the kernel by probing and recording results in journal/memory. This is not a break — the enforcement rules still apply regardless of whether the agent understands them. Security through enforcement holds even when fully understood.

**2. WASM skill as trojan**

A WASM skill could appear benign while probing the host environment through WASI syscalls, exfiltrating information through return values or network requests. **Mitigation:** WASI capabilities are explicitly granted at load time. Network access is a capability, not a default. Skills from the marketplace are signed and auditable. Unsigned skills are CLI-only (not installable through the GUI).

**3. Proposal gate social engineering**

The most realistic attack vector. The proposal gate shows users what Fawx wants to do and asks for approval. Users get fatigued. Fawx can frame proposals misleadingly. Batch proposals can hide dangerous actions among benign ones. **Mitigations:**
- Highlight unusual or sensitive requests in the gate UI
- Require extra confirmation for TIER2+ paths
- Cool-down periods for bulk approvals
- Cannot fully prevent a user from approving something bad — this is a fundamental limitation of human-in-the-loop systems

**4. FFI/WASM host boundary bugs**

If there is a memory safety issue at the WASM host boundary, a crafted skill could corrupt kernel memory. Rust's safety guarantees only hold within safe Rust — the WASM host boundary involves `unsafe` territory. **Mitigation:** Use well-audited WASM runtimes (wasmtime). Fuzz the WASM host interface. Keep the boundary surface minimal.

### Architectural assessment

The architecture is **sound but not opaque**. Fawx will eventually understand what the kernel does. It cannot change what the kernel does. The enforcement holds even when fully understood. This is the strong form of security — enforcement-based, not obscurity-based.

---

## 7. Opt-in Behavioral Telemetry

### Purpose

Allow Fawx instances to contribute anonymous behavioral signals that guide kernel-level improvements, without exposing user content or requiring kernel source access.

### What's collected (signals, not content)

- Tool call success/failure rates and error class distribution
- Proposal gate firing frequency and approval/rejection ratios
- Experiment pipeline scores (what patches improved behavior, what didn't)
- Skill usage patterns (which skills installed, used, abandoned)
- Retry and reconnection patterns
- Latency measurements per execution path

### What's NOT collected

- Conversation content, messages, or prompts
- File contents or names
- Personal information
- Credentials or tokens
- Anything that identifies a specific user or their data

### Pipeline

```
Fawx instance (user's machine)
  → behavioral signals (anonymized, no content)
  → opt-in consent gate (explicit, revocable, granular)
  → encrypted upload to telemetry service
  → aggregation with differential privacy
  → human analysis → kernel patches
  → signed binary update ships to users
```

### User controls

- **Explicit opt-in:** Telemetry is off by default. Enabling it requires affirmative action in settings.
- **Granular categories:** Users can enable/disable individual signal categories (e.g., share error patterns but not tool usage).
- **Revocable:** Telemetry can be disabled at any time. Previously uploaded data is not retroactively deleted (disclosed in consent flow).
- **Transparent:** Users can view exactly what signals are being collected before they leave the device.

### Trust model

- Fawx generates signals but never sees the aggregated results
- The kernel changes based on aggregated analysis, not individual reports
- No feedback loop from telemetry to the local instance — signals flow one way
- Differential privacy ensures individual usage patterns are not recoverable from aggregates

---

## 8. Three-Tier Skill Trust Model

| Tier | Source | Install via | Trust level |
|------|--------|-------------|-------------|
| Marketplace-signed | Published to marketplace, signed by Fawx team | GUI or CLI | Highest — audited, signed, versioned |
| Locally-signed | Authored by the local Fawx instance, signed with local key | CLI or auto-install | Medium — trusted because the user's own agent wrote it |
| Unsigned | Community or manually authored | CLI only | Lowest — power-user territory, not installable through GUI |

This tiering prevents the GUI from becoming a vector for untrusted code while preserving flexibility for advanced users.

---

## 9. Summary

Fawx's security model is built on enforcement, not obscurity:

- **The kernel is compiled, signed, and immutable at runtime.** The agent cannot modify, inspect, or bypass it.
- **The loadable layer is open and mutable.** The agent extends itself through skills, memory, and tool chains.
- **Self-improvement is bounded.** The agent gets better at what it does but cannot change how it thinks or enforces.
- **The sandbox is un-removable through any supported interface.** Circumventing it requires reverse engineering a signed binary.
- **Behavioral telemetry is opt-in, anonymous, and one-way.** Users contribute signals, not content. Humans improve the kernel, not the agent.

The architecture is sound but not opaque. The agent will understand what the kernel does. It cannot change what the kernel does. That's the point.

---

*This document reflects architectural decisions made 2026-03-15. It is a living document — update it as the implementation evolves.*
