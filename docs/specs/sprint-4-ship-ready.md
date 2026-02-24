# Sprint 4: Ship Ready

*From "works for Joe" to "works for anyone."*

**Status:** Planning
**Authors:** Joe, Clawdio
**Date:** 2026-02-24
**Prerequisite:** Sprints 0-3 complete

---

## Thesis

Citros has a working phone agent with durable execution, loop tuning, action playbooks, and a safety layer. Four sprints of architecture — done. But there's a gap between "works on Joe's phone" and "works on anyone's phone":

1. **Token bloat kills cheaper models.** Every turn sends all tools and full prompts regardless of context. Haiku chokes on the payload. Sonnet wastes tokens on irrelevant tool descriptions. The agent is expensive to run and can't scale down to budget-friendly models.

2. **Known workflows break.** Google Maps gets stuck on suggestion dropdowns. Third-party text input garbles output. Voice input dies on clean install. These aren't edge cases — they're top-5 workflows. Users hit them in the first 10 minutes.

3. **Test infrastructure is fragile.** The CI gate is weakened (#751), the regression harness is a skeleton with no real scenarios, and Sprint 3 backlog items erode safety layer confidence. You can't ship what you can't regression-test.

4. **First-run is a wall.** Install the app, stare at a settings screen, figure out API keys, configure a provider. Zero guidance. The "zero-infra" promise dies in the first 60 seconds.

---

## Sprint Map

```
Stream A: H2 Completion              Stream B: Reliability
──────────────────────               ─────────────────────
Tool Grouping (#557)                 Google Maps fix (#647)
Prompt Tuning (#558)                 Text input fallback (#638)
Token reduction, model-tier          Voice input fix (#663)
  budget enforcement                 Regression suite hardening
          │                                    │
          │              Stream C: Test Infra   │
          │              ───────────────────    │
          │              CI baseline (#751)     │
          │              Sensor CI (#698)       │
          │              Safety backlog         │
          │              (#779, #780)           │
          │                    │                │
          └────────────┬───────┘────────────────┘
                       │
               Stream D: Onboarding
               ────────────────────
               Conversational setup (#596)
               Key wallet UX (#470)
               Zero-infra first-run
```

## Stream Details

| Stream | Focus | Issues | Spec | Node | Est. PRs |
|--------|-------|--------|------|------|----------|
| **A1: Tool Grouping** | Runtime category selection, user prefs, token reduction | #557, #680 | `h2-3-tool-grouping-spec.md` ✅ | Mac Mini | 3-4 |
| **A2: Prompt Tuning** | Budget enforcement, tier-based sections, rollout safety | #558, #681, #682 | `h2-4-model-aware-prompt-tuning-spec.md` ✅ | Mac Mini | 3-4 |
| **B: Reliability** | Bug fixes for top workflows + regression suite flesh-out | #647, #638, #663, harness | `sprint-4-reliability.md` 🆕 | MacBook Pro | 4-5 |
| **C: Test Infra** | CI stabilization + Sprint 3 safety backlog | #751, #698, #779, #780 | Folded into B (PR 5) | MacBook Pro | 1 (combined) |
| **D: Onboarding** | Conversational first-run + key wallet + zero-infra flow | #470, #596, #571 | `sprint-4-onboarding.md` 🆕 | Either | 3-4 |

### Parallelization Plan (2 nodes)

| Node | Cores | Streams | Max Parallel Squads |
|------|-------|---------|-------------------|
| **Mac Mini** (10c/16GB) | A1 + A2 | 2 squads (independent specs) |
| **MacBook Pro** (8c/16GB) | B + C + D | 3 squads (B+C share worktree, D independent) |

**Total: up to 5 parallel squads.** B and C can share a worktree since C's backlog items touch different files than B's bug fixes. D is fully independent.

### Dependencies

- A1 and A2 are **independent** of each other and everything else.
- B and C are **independent** — different files, different concerns.
- D is **independent** — onboarding UI is separate from core engine.
- No stream depends on another stream's output.

This is the most parallelizable sprint yet.

---

## Success Criteria (End of Sprint 4)

| Metric | Current | Target |
|--------|---------|--------|
| Tool payload size (avg turn) | All tools, all turns | 40-60% reduction via grouping |
| Prompt token budget | Unbounded | Hard limit per tier, deterministic trim |
| Haiku task success rate | Poor (payload too large) | Viable for simple tasks |
| Google Maps search | ❌ Stuck on dropdowns | ✅ Completes reliably |
| 3rd-party text input | ❌ Garbled | ✅ Fallback chain works |
| Voice input (clean install) | ❌ Stops immediately | ✅ Falls back to Android STT |
| Regression suite scenarios | 0 real scenarios | 10+ top workflows |
| CI gate | Weakened | Full green gate on staging |
| First-run time to first task | ~5 min (manual config) | <2 min (guided) |
| Onboarding drop-off | Unknown (no tracking) | Measurable funnel |

## Latency Budget

| Operation | Target (p95) | Notes |
|-----------|-------------|-------|
| Text input chain (all tiers) | < 500ms | Tier 3 char-by-char is slowest path |
| Onboarding state check | < 50ms | SharedPreferences read, no I/O |
| Model recommendation | < 3s | Network call to provider API |
| Maps search submission | < 2s | Type + IME action + verify |

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| Tool grouping breaks existing tasks | High | Feature-flagged rollout + regression suite |
| Prompt budget trims safety text | Critical | Safety sections never trimmed (invariant) |
| Maps fix is app-version-dependent | Medium | IME search action (Enter/`IME_ACTION_SEARCH`) as primary approach — device-independent. Spatial tap fallback uses density-aware dp offsets, not hardcoded pixels. |
| Voice fix requires native lib update | Medium | Fallback to Android STT with explicit user consent (privacy choice: cloud STT / type instead / wait for download) |
| Onboarding scope creep | Low | MVP: conversation + key entry. No account system. |

---

*Individual stream specs follow. Streams A1 and A2 have existing specs. Streams B and D have new specs below.*
