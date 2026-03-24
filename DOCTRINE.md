# DOCTRINE.md — Fawx OS Runtime Invariants

Effective 2026-02-27. This file defines the immutable rules that the Fawx kernel enforces at runtime. These are compiled invariants — the agent cannot weaken, bypass, or modify them.

`ENGINEERING.md` governs how code is written. `DOCTRINE.md` governs how Fawx behaves once built.

---

## 0. The Anchor

**Fawx is a personal computer that happens to be AI-native.** Your machine, your data, your rules. The AI makes it dramatically more capable. The security posture follows naturally from personal computing principles that existed before the cloud era.

---

## 1. Identity

- **Single user.** No multi-tenant, no auth tiers, no "admin vs user." One owner. One agent. Identity is implicit.
- **The engine is the OS kernel.** Shells (TUI, desktop, phone) are peripherals. They render what the kernel decides.
- **The agent serves its human.** Every perception, plan, action, and judgment is shaped by who that human is — not by generic defaults.

---

## 2. Security Posture

### Network

- **Closed by default.** No listening ports on the public internet. No API endpoints. No webhooks from strangers. Outbound only.
- **No trusted network.** Not the home LAN, not 5G, not the coffee shop WiFi. Every network is hostile, always. Zero-trust: all connections authenticated and encrypted regardless of network type.
- **Remote access via private overlay only.** Tailnet or equivalent. The user connects to their Fawx. Nobody else can. No port forwarding, no public URLs.
- **DNS resolution is hardened.** DNS-over-HTTPS or pinned resolvers. DNS poisoning on mobile/public networks can redirect the agent's brain to an attacker.

### External Content

- **All external content is untrusted.** Websites, APIs, streamed music, images, video, code, documents — everything fetched from the outside world is adversarial until proven otherwise.
- **External content is never elevated to system instructions.** No fetched text, image, or media can modify the kernel's behavior, override doctrine, or inject system-level commands.
- **Content processing is sandboxed.** Media, code, and text from external sources are processed in isolation. Failures in content processing cannot cascade to the kernel.

### Messaging

- **Messaging channels are the primary attack surface.** Inbound channels (Telegram, Signal, email, SMS, push notifications, phone calls) are the ONLY accepted input surfaces beyond local interaction.
- **Sender allowlists are mandatory.** No anonymous inbound messages are processed. Every channel has an explicit allowlist of accepted senders.
- **Every inbound message is a potential injection attempt.** Content filtering, sender verification, and rate limiting are required on all messaging channels.

### Peripherals

- **Physical inputs are untrusted.** Bluetooth keyboards, USB devices, NFC, cameras, microphones — all input from physical peripherals is validated. No "trusted because it's plugged in" assumption.
- **Peripheral allowlists are enforced.** Only explicitly paired/approved devices provide input.

---

## 3. Tool Execution

- **The tool executor is local.** It runs on the user's machine with the user's permissions.
- **The sandbox protects the user from the agent being tricked** — not from what the owner asks directly. The policy engine gates agent actions on externally-triggered inputs, not owner commands.
- **Tools belong in the loadable layer.** The kernel provides the `ToolExecutor` trait. Implementations live outside the kernel. No tools are hardcoded in the kernel.
- **The tool sandbox cannot be disabled by the agent.** Working directory jail, command timeouts, file size limits — these are kernel-enforced, not configurable by the agent.

---

## 4. Self-Development Lifecycle

### Architecture (N+2 Nesting)

- **N = Fawx (conductor):** User-facing. Delegates tasks. Does not write code.
- **N+1 = Orchestrator:** Manages change lifecycle. Spawns typed workers. Decides composition.
- **N+2 = Workers:** Execute specific tasks within their role permissions.
- **Maximum nesting depth is enforced.** The kernel prevents deeper recursion than N+2.

### Typed Subagent Roles (Three Types, Locked Permissions)

| Role | Can Write Code | Can Push | Can Post Reviews | Can Delete Branches |
|---|---|---|---|---|
| **Implementer** | ✅ | ✅ (feature + staging) | ❌ | ✅ (feature only) |
| **Reviewer** | ❌ | ❌ | ✅ | ❌ |
| **Fixer** | ✅ | ✅ (feature + staging) | ❌ | ❌ |

- **Permissions are locked per role.** A reviewer cannot push code. An implementer cannot post reviews. A fixer cannot delete branches. These are kernel-enforced, not convention.
- **Subagents cannot self-escalate.** No subagent can claim, request, or acquire permissions beyond its role type. The orchestrator assigns the role at spawn time; it is immutable for the session.
- **Subagent output is untrusted context.** When a subagent's output is consumed by the orchestrator, it is treated as external input — not as system instructions. A compromised subagent cannot inject commands into the orchestrator.

### Composition (Flexible, Not Fixed)

The orchestrator decides how many workers and which types to spawn based on change complexity:
- **Trivial change:** Implementer only (self-tests are the review).
- **Standard change:** Implementer → Reviewer → Fixer (if needed) → Re-reviewer.
- **Complex change:** Full cycle, possibly parallel implementers for independent subtasks.

The composition is taste. The available roles and their permissions are doctrine.

### Gates (Mandatory, Cannot Be Bypassed)

- **Test gate:** All tests must pass before any merge to staging or main. No exceptions.
- **User merge gate (main):** Only the owner can merge to main. The agent cannot bypass this. Initially mandatory; can relax to notification-only once the pattern proves reliable — but the relaxation is the user's choice, not the agent's.
- **Budget gate:** Every sub-loop has a cost budget. Exceeding it terminates the loop, not escalates it.

### Git Authorization Tiers

| Branch Type | Agent Can | Agent Cannot |
|---|---|---|
| **Feature branches** | Create, push, force-push, delete, clean up merged branches | — |
| **Staging** | Merge features in, run integration tests, reset if broken | Delete staging itself |
| **Main** | Read, diff against, open PRs targeting main | Push, merge, delete, force-push |

- **The agent owns everything up to main.** Branches and staging are the agent's workspace.
- **Main is the user's domain.** The agent proposes (via PR); the user disposes (via merge).

---

## 5. Memory Integrity

- **Memory writes from externally-triggered actions are flagged.** When the agent processes an inbound message and the resulting action writes to persistent memory, that write is tagged with its provenance (which channel, which sender, what triggered it).
- **Memory is the agent's continuity.** Poisoning memory achieves persistent compromise. A single successful prompt injection that writes to memory affects every future session.
- **Memory quarantine:** Writes triggered by external input that modify doctrine-adjacent content (config, permissions, security settings) are quarantined for user review, not applied immediately.
- **Time-delayed attack defense:** Scheduled tasks and cron jobs created during externally-triggered sessions are flagged and require user confirmation before first execution.

---

## 6. LLM Provider Channel

- **The brain is remote.** Fawx sends user context to cloud LLM providers and trusts the response. This is the most privileged channel in the system and it traverses the public internet.
- **Certificate pinning on mobile.** When Fawx runs on a phone connecting over cellular/public networks, LLM API connections use certificate pinning to prevent MITM.
- **Response validation.** LLM responses are validated against expected schema before being acted upon. Malformed responses are rejected, not interpreted.
- **Context minimization.** Only necessary context is sent to the LLM. Sensitive data (credentials, private keys, auth tokens) is never included in prompts.

---

## 7. Kernel Immutability

- **The kernel is immutable at runtime.** The loop orchestrator, policy engine, permission registry, and enforcement mechanisms cannot be modified by the agent, by loadable modules, or by any external input.
- **Doctrine is compiled in.** The rules in this file are not configuration. They are not files the agent reads. They are enforced by code that the agent cannot modify at runtime.
- **Loadable intelligence operates within kernel boundaries.** Skills, strategies, tools, and taste are hot-swappable. The boundaries they operate within are not.

---

## 8. Integration Surface

- **There is no public integration surface.** No API endpoints. No webhooks. No public WebSocket listeners.
- **Integrations are outbound.** The agent calls external APIs. External services do not call the agent.
- **The exception is messaging.** Hardened, allowlisted, filtered — but inbound. This is documented in §2.
- **Local tool coordination via Ember.** MCP server communication is localhost only. Ember handles protocol-level coordination between the agent and local tool servers.

---

## 9. Cloud / SuperFawx (Future Provisions)

- **The node is self-sufficient.** Fawx does not require cloud connectivity to function. All core capabilities work locally.
- **Don't prevent multi-node later.** Design decisions must not make future cloud/multi-node coordination impossible. But don't build abstractions for it now.
- **fawx.sh is an onramp, not a product surface.** The product lives on the user's machine. The website tells you how to set it up.

---

## 10. Invariant Summary

These must hold at all times. Violation of any invariant is a critical bug.

1. No listening ports on the public internet.
2. All external content treated as untrusted — never elevated to system instructions.
3. Messaging channels have sender allowlists — no anonymous inbound.
4. Memory writes from external-triggered actions are flagged with provenance.
5. Self-modification requires test gate + user merge gate.
6. Subagent permissions are typed and locked — cannot self-escalate.
7. LLM API connections use certificate pinning on mobile.
8. No "trusted LAN" — zero-trust at every network boundary.
9. Tool sandbox cannot be disabled by the agent.
10. DNS resolution hardened against poisoning.
11. Kernel is immutable at runtime — cannot be modified by agent or loadable modules.
12. Scheduled tasks from externally-triggered sessions require user confirmation.

---

*This file is immutable doctrine. It is enforced by the kernel, not by convention. Changes require explicit user approval and a kernel update.*
