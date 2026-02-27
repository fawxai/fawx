# Security Model Scoping — Pre-Compaction Save

This file captures the security/integration design discussion from 2026-02-27 for use in writing `docs/architecture/security-model.html`.

---

## Joe's 9 Principles (Verbatim)

1. Fawx is meant to serve an individual user.
2. Fawx is meant to be an OS designed around AI in a way that harnesses the AI's power to serve the user in the way that best suites their user.
3. Can the internet openly interact with a personal OS? No. It is mostly a one way OS -> Internet flow, where information is pulled back to the OS when permitted. And it should probably stay that way. Maybe an individual wants to reach their fawx computer remotely, that would be the exception, but the rule to that exception is that only the individual user for that Fawx should be able to reach it.
4. With #3 in mind, Fawx OS should be able to pull information over a web connection, but only for actions that are permitted by the user.
5. One major exception to the above: the personal messaging and notification systems from telephony, messaging interfaces, email, and push notifications. This is where we need to be extremely careful about our design choices, because we don't want to hinder these capabilities, but they are inherently attack surfaces that we must harden at all costs.
6. With #5 in mind, Fawx OS should be able to receive input from accepted sources but only in secure ways with hardened filtering against attack vectors.
7. An individual user should be able to reach their Fawx remotely, but in a secure way over a tailnet, or something similar.
8. Users should eventually be able to have a cloud storage for Fawxes, and maybe a SuperFawx that talks to many remote Fawx nodes, but do we need to design for that right away? Can we start with the node, and build for the Cloud later?
9. For now, fawx.sh/fawx.ai are the landing pages that funnel users to set up their nodes. Eventually, it could become a path for distributed/decentralized cloud access, but as mentioned in #8, I'm not sure we need that right away.

---

## Architectural Mapping (from discussion)

| Principle | Decision |
|---|---|
| #1 Single user | No multi-tenant, no auth tiers. One owner. One agent. Identity is implicit. |
| #2 AI-native OS | The engine IS the OS kernel. Shells (TUI, desktop, phone) are just peripherals. |
| #3 Closed by default | No listening ports on public internet. No API endpoints. No webhooks from strangers. Outbound only. |
| #4 Pull with permission | Tool execution (web fetch, API calls) requires user-defined policy. Agent reaches out only where user permits. |
| #5-6 Messaging exception | Inbound channels (Telegram, Signal, email, push) are the ONLY accepted input surfaces beyond local. Each channel has allowlists, sender verification, content filtering. THIS IS THE ATTACK SURFACE. |
| #7 Remote access | Tailnet (or equivalent private overlay). Only the user connects. No port forwarding, no public URLs. |
| #8 Cloud is later | Design node to be self-sufficient. Don't add multi-node abstractions yet. Don't prevent it later. |
| #9 Landing page → node | fawx.sh is an onramp, not a product surface. The product lives on the user's machine. |

---

## Key Insights

- **Web interface:** There isn't one in the traditional sense. No fawx.com dashboard. TUI/desktop shell IS the interface. Web shell (if built) connects to YOUR node over YOUR tailnet.
- **Tool execution security:** The sandbox protects the user from the agent being TRICKED by external input (#5-6), not from what the owner asks directly.
- **Integration surface:** No public integration surface. Integrations are outbound (agent calls APIs) or through hardened messaging channels. Ember handles protocol-level tool coordination — all localhost.
- **THE threat model:** Attackers can't SSH in, can't hit an API, can't open a WebSocket — but they CAN send a message that the agent processes. Prompt injection via messaging channels is the primary attack vector.

---

## Self-Development Agent Squad (Architectural Decision)

### N+2 Nesting Structure
- N = Fawx (conductor) — user-facing, delegates tasks
- N+1 = Orchestrator — manages PR lifecycle, spawns workers
- N+2 = Workers — do the actual work

### Three Typed Subagent Roles
- **Implementer**: can write code, push, run tests. Cannot post reviews.
- **Reviewer**: read-only, can post reviews. Cannot push code.
- **Fixer**: can write code, push. Gets review findings as context.
- Each type has locked-down permissions + standardized output format.

### Flexible Composition
- Orchestrator decides how many workers and which types based on change complexity
- Trivial: implementer only. Standard: implementer → reviewer → fixer. Complex: full cycle + parallel.

### Archetype vs Loadable Split
**Archetype (compiled, immutable — DOCTRINE):**
- N+2 nesting structure enforced
- Three typed roles with locked permissions
- Mandatory test gate before any merge
- User merge gate (cannot be bypassed)
- Git lifecycle invariants (branch protection, no direct push to main)
- Budget/depth limits on sub-loops

**Loadable (hot-swappable — TASTE):**
- How to decompose a task into subtasks
- What context to include in worker prompts
- When orchestrator handles directly vs spawning workers
- ENGINEERING.md rules content
- Prompt templates for each role
- Complexity assessment heuristics

### Git Authorization Tiers
| Branch | Agent Can | Agent Cannot |
|---|---|---|
| Feature branches | Create, push, delete, force-push, clean up | — |
| Staging | Merge features in, run integration, reset if broken | Delete staging |
| Main | Read, diff against | Push, merge, delete |

---

## DOCTRINE.md vs TASTE.md for Fawx OS

ENGINEERING.md and TASTE.md (already in repo) cover DEVELOPMENT PROCESS standards.

DOCTRINE.md and OS-level TASTE need to cover the RUNTIME SYSTEM rules:
- Kernel invariants (immutable at runtime)
- Security posture (closed by default, messaging exception)
- Permission model (single user, tool execution policy)
- Self-development lifecycle archetype (N+2, typed roles, merge gates)
- Git authorization tiers

These get "wired into the system programming" — they become the actual rules the Fawx kernel enforces at runtime, not just docs developers read.
