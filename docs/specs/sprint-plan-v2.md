# Sprint Plan v2 — From Demo to Product

*Four sprints from "works when Joe watches" to "works on anyone's phone."*

**Status:** Planning
**Authors:** Joe, Clawdio
**Date:** 2026-02-24

---

## Thesis

Citros has a working phone agent. The agentic loop executes tasks, voice I/O works, memory persists, the UI is polished. But it's a demo, not a product. Three structural gaps prevent it from being something you'd hand to another human:

1. **The brain dies when the activity dies.** The agent loop runs in `ChatViewModel`, scoped to `ChatActivity`. Switch apps, screen off too long, memory pressure — the loop dies mid-task with no recovery. No amount of loop tuning fixes this if the process hosting the loop gets killed.

2. **Every execution is exploration.** The agent navigates from scratch every time. "Send a text" = explore messaging app UI, find compose, find recipient field, figure out send button. Every. Single. Time. A human learns the path once and follows it forever. The agent should too.

3. **No safety boundary between model output and phone execution.** The LLM's tool requests execute directly. No confirmation for sending messages, no block on financial actions, no rate limiting. One bad model output can send money or messages.

These map to four sprints, each building on the last:

---

## Sprint Map

```
Sprint 0: Service Architecture       Sprint 1: Loop Tuning
─────────────────────────            ────────────────────
Move brain to foreground service.    Subtask decomposition.
Durable task state.                  Deterministic recovery.
Survive app switches + screen off.   Better first-execution success.
                    │                            │
                    └───────────┬────────────────┘
                                │
                    Sprint 2: Action Playbooks
                    ──────────────────────────
                    Record successful executions.
                    Parameterized templates.
                    Replay > explore.
                    Logarithmic cost curve.
                                │
                    Sprint 3: Safety Layer
                    ──────────────────────
                    HITL policy engine.
                    Confirmation gates.
                    Financial deny, rate limits.
                    Audit trail.
```

## Sprint Details

| Sprint | Spec | Focus | Depends On | PRs Est. |
|--------|------|-------|------------|----------|
| **0: Service Architecture** | `sprint-0-service-architecture.md` | Move AgentExecutor to foreground service, durable task state, wake lock management, service binding | — | 3-4 |
| **1: Loop Tuning** | `sprint-1-loop-tuning.md` | Subtask decomposition, deterministic recovery behaviors, regression harness skeleton | Sprint 0 | 3-4 |
| **2: Action Playbooks** | `sprint-2-action-playbooks.md` | Screen fingerprinting, playbook recording, parameterized templates, playbook-guided execution | Sprint 1 | 3-4 |
| **3: Safety Layer** | `h2-action-policy-engine.md` (updated) | HITL policy engine, state-dependent pill buttons, confirmation UI, audit, rate limiting, egress control | Sprint 0 | 5-7 |

**Sprint 3 spec updated** (`h2-action-policy-engine.md`) with §8a State-Dependent Pill Buttons — contextual UI pills that replace generic Allow/Deny with state-aware options (Authenticate, Continue, Do something else, etc.) and an `offer_choices` tool for agent-generated disambiguation pills.

**Sprint 1 and 2 are sequential** — playbooks built on unreliable first executions produce bad playbooks. Loop tuning first, then record from higher-quality executions.

**Sprint 0 and 3 are independently sequenced** — the service architecture (Sprint 0) is a prerequisite for both loop tuning and the safety layer, but Sprint 3 doesn't depend on Sprint 1 or 2.

## Success Criteria (End of Sprint 3)

| Metric | Current | Target |
|--------|---------|--------|
| Agent survives app switch | ❌ Dies | ✅ Continues |
| Agent survives screen off | ❌ Dies | ✅ Continues |
| Complex multi-step task success | ~50% | >80% |
| Repeat task speed (2nd+ execution) | Same as first | 2-3x faster |
| Repeat task API cost | Same as first | Near-zero (playbook) |
| Dangerous action gate | None | Confirm/Deny enforced |
| Financial action protection | None | Hard deny (Phase 1) |

## Moat Analysis

After all four sprints, Citros has three compounding moats:

1. **Playbook library** — every successful execution makes the next one faster and cheaper. Scales to community sharing (H3.7). Cold-start advantage grows with time.
2. **HITL policy engine** — per-invocation context-aware safety that no other phone agent has. OpenClaw has static tool allowlists; Citros has runtime policy evaluation with screen context, sensitive app detection, and financial submit blocking.
3. **Service persistence** — the agent survives real-world Android lifecycle events. It's not a demo that works when you're watching — it works when you're not.

---

*Individual sprint specs follow. Each is self-contained with architecture, data models, integration points, test matrix, and blindspots.*
