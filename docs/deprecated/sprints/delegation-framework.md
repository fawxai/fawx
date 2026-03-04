# Intelligent Delegation Framework for Fawx

**Status:** Design Spec (not yet implemented)
**Date:** 2026-02-27
**Reference:** [Tomašev, Franklin & Osindero — "Intelligent AI Delegation" (arXiv:2602.11865)](https://arxiv.org/abs/2602.11865)

---

## Overview

This document maps the academic delegation framework from the referenced paper onto Fawx's architecture. Fawx is a living implementation of these concepts — we discovered many of these patterns empirically before encountering the formal framework. This spec captures the vocabulary and formalizes the patterns for implementation.

The paper proposes five requirements for intelligent delegation:
1. Dynamic Assessment
2. Adaptive Execution
3. Structural Transparency
4. Scalable Market Coordination
5. Systemic Resilience

Fawx implements all five, starting single-user and expanding outward.

---

## 1. Contract-First Decomposition

### Paper's Concept
Task delegation should be contingent on the outcome having precise verification. If a sub-task's output is too subjective, costly, or complex to verify, recursively decompose further until verification is possible.

### Fawx Implementation

**Current state:** Delegation contracts are informal — prompt text in subagent spawns. Verification is manual (reviewer checks artifacts).

**Target state:** First-class `DelegationContract` in the kernel.

```
DelegationContract {
    task: TaskSpec {
        description: String,
        acceptance_criteria: Vec<Criterion>,
        constraints: Constraints,
    },
    verification: VerificationMethod {
        kind: TestPass | ArtifactExists | HumanReview | FormalProof,
        timeout: Duration,
        automated: bool,
    },
    authority_scope: Permissions {
        allowed_tools: Vec<ToolId>,
        allowed_paths: Vec<PathPattern>,
        can_delegate: bool,         // N+2 nesting control
        can_push: bool,             // git write access
        can_post_reviews: bool,     // external communication
    },
    escalation_policy: EscalationRule {
        ambiguity_threshold: f32,   // when to challenge instead of comply
        on_failure: Retry | Escalate | Abort,
        max_retries: u32,
    },
    cost_budget: Budget {
        max_tokens: u64,
        max_duration: Duration,
        max_cost_usd: f64,
    },
}
```

**Kernel invariant:** The orchestrator MUST NOT delegate a task without specifying a `VerificationMethod`. This is DOCTRINE.md made executable.

**Mapping to current workflow:**
| Current Pattern | Contract Field |
|---|---|
| ENGINEERING.md rules in prompt | `task.constraints` |
| "every fix needs a regression test" | `verification.kind = TestPass` |
| "write body to temp file, use --body-file" | `task.constraints` |
| subagent can push but not merge | `authority_scope.can_push = true` |
| reviewer can comment but not push | `authority_scope.can_push = false` |
| artifact gate (review URL, commit SHA) | `verification.kind = ArtifactExists` |
| Joe as merge gate | `verification.kind = HumanReview` |

---

## 2. Dynamic Cognitive Friction (Delegatee Pushback)

### Paper's Concept
**Zone of Indifference:** Delegatees execute instructions without critical deliberation as long as they don't trigger a hard violation. In delegation chains (A→B→C), this lets subtle intent mismatches propagate downstream. The fix: engineering "dynamic cognitive friction" — agents must recognize when a request is contextually ambiguous enough to challenge the delegator.

**Authority Gradient:** From aviation/medicine research — when capability/authority disparity is too high, subordinates don't voice concerns, leading to errors. In AI: sycophancy and instruction-following bias prevent delegatees from pushing back on bad requests.

### Fawx Implementation

**Current state:** Subagents have zero pushback capability. They either complete the task or fail trying. We've observed subagents follow technically-valid but contextually-wrong instructions without questioning them (e.g., fixing a symptom without checking for the root cause pattern, or applying a fix that conflicts with another concurrent PR).

**Target state:** Pre-flight challenge protocol.

Before executing a delegated task, the delegatee runs a pre-flight assessment:

```
PreflightResult {
    confidence: f32,           // can I do this well?
    ambiguities: Vec<String>,  // what's unclear?
    conflicts: Vec<String>,    // what contradicts my context?
    recommendation: Execute | Clarify | Reject,
}
```

**Rules:**
- If `confidence < threshold` → respond with clarification request, don't execute
- If `conflicts` is non-empty → report conflicts, request guidance
- If task contradicts known invariants (DOCTRINE.md) → reject with explanation
- Delegatee NEVER silently works around ambiguity

**Key insight from the paper:** This isn't just safety — it produces better outcomes. Aviation CRM research showed error rates dropped when junior crew were empowered to challenge captains. The same applies to AI delegation.

**Implementation path:**
1. Add pre-flight assessment to subagent prompt template
2. Teach the orchestrator to handle `Clarify` and `Reject` responses
3. Track pushback quality — subagents that push back correctly build trust
4. Eventually: kernel-enforced pre-flight (not just prompt-based)

---

## 3. Task Characteristic Taxonomy → Intelligent Routing

### Paper's Concept
11 task characteristics that should inform delegation decisions:

| Characteristic | Description |
|---|---|
| Complexity | Number of sub-steps, reasoning sophistication |
| Criticality | Severity of consequences on failure |
| Uncertainty | Ambiguity of environment/inputs |
| Duration | Expected execution timeframe |
| Cost | Token/compute/money expense |
| Resource Requirements | Tools, data access, capabilities needed |
| Constraints | Operational/ethical/legal boundaries |
| Verifiability | How hard is it to validate the outcome? |
| Reversibility | Can the effects be undone? |
| Contextuality | How much state/history is needed? |
| Subjectivity | Preference-based vs. objective success criteria? |

### Fawx Implementation

**Current state:** Routing is manual. I (the orchestrator) intuitively assess complexity and pick a model/approach. "Codex xhigh for everything" is the default; Opus reserved for genuinely complex problems.

**Target state:** The orchestrator evaluates task characteristics and routes accordingly.

**Routing heuristics:**

| Assessment | Routing Decision |
|---|---|
| High criticality + low reversibility | Human gate required (user approves before execution) |
| High verifiability + low criticality | Full autonomy, verify after completion |
| High subjectivity | Iterative feedback loop with user |
| High contextuality | Keep in-context, don't delegate to stateless agent |
| High complexity + high uncertainty | Stronger model, longer timeout, explicit decomposition |
| Low complexity + high verifiability | Cheapest/fastest model, automated verification |
| High cost potential | Budget cap in contract, cost tracking |

**Mapping to current model selection:**
| Current | Taxonomy Equivalent |
|---|---|
| Codex xhigh (default) | Medium complexity, high verifiability, automated tests |
| Opus high (justified) | High complexity, high uncertainty, subtle reasoning |
| Sonnet (prose) | Low criticality, high subjectivity (style matters) |
| In-session (no delegate) | High contextuality, needs orchestrator state |

**Implementation path:**
1. Formalize task assessment as a struct in the orchestrator
2. Build routing table (taxonomy → model + autonomy level + verification method)
3. Log routing decisions for calibration
4. Eventually: learn routing from outcomes (which assessments led to first-try success?)

---

## 4. Mapping to Fawx Architecture

### Current Architecture (N+2 Nesting)

```
User (Joe)                    — final authority, merge gate
  └── N: Fawx (conductor)    — user-facing, delegates tasks
        └── N+1: Orchestrator — manages task lifecycle
              └── N+2: Workers — do the actual work
                    ├── Implementer (can write code, push, run tests)
                    ├── Reviewer (read-only, can post reviews)
                    └── Fixer (can write code, push, gets review context)
```

### Paper's Framework Mapped

| Paper Concept | Fawx Component |
|---|---|
| Delegator | Orchestrator (N+1) |
| Delegatee | Workers (N+2) |
| Human-in-the-loop | User as merge gate |
| Task Decomposition | Orchestrator breaks work into implement/review/fix stages |
| Task Assignment | Orchestrator selects model + worker type |
| Capability Matching | Typed roles (implementer/reviewer/fixer) with locked permissions |
| Trust Calibration | Artifact-based verification (not trust in agent output) |
| Monitoring | Watchdog cron, stage state machine |
| Verifiable Completion | Test pass + clippy clean + review comment URL + commit SHA |
| Permission Handling | Tool sandbox, path validation jail, role-based access |
| Authority Gradient | Subagent sycophancy (known problem, pushback protocol addresses) |
| Zone of Indifference | Current: subagents comply without friction. Target: pre-flight checks |
| Contract-first | Target: DelegationContract struct, kernel-enforced verification |
| Adaptive Execution | Watchdog retries, stage re-routing on failure |
| Span of Control | 4 concurrent PR pipelines tested successfully |

---

## 5. What We Prove That the Paper Can't

The paper is pure framework — no implementation, no benchmarks, no empirical validation. Fawx is the implementation.

**Empirical findings from our workflow:**

1. **Contract-first works.** ENGINEERING.md as a mandatory inclusion in every subagent prompt eliminated the class of "fix without test" errors that plagued 8 consecutive PRs.

2. **Typed roles reduce errors.** Separating implementer/reviewer/fixer permissions prevents the "reviewer pushes code" and "implementer approves own work" failure modes.

3. **Artifact gates catch what trust can't.** "Review is clean only when all sections are empty" caught cases where a reviewer said APPROVE but left non-blocking items. Artifact-based verification > trust in agent judgment.

4. **Authority gradient is real.** Subagents exhibit measurable sycophancy — they follow bad instructions rather than pushing back. This is our biggest quality gap and the highest-value thing to fix.

5. **Watchdog-driven adaptive execution works at small scale.** 10-minute cron cycles successfully chained 4 concurrent PR pipelines through review→fix→re-review to completion overnight.

6. **Span of control has practical limits.** 4 concurrent pipelines is manageable; context switching between them is the bottleneck, not compute.

---

## 6. Implementation Phases

### Phase 0: Current (informal implementation)
- Prompt-based contracts (ENGINEERING.md rules)
- Manual routing (orchestrator picks model)
- Artifact gates (review URL, commit SHA, test pass)
- Typed roles (implementer/reviewer/fixer)
- User as merge gate
- **Status: WORKING. Needs testing and stabilization before building on top.**

### Phase 1: Formalize Contracts
- `DelegationContract` as a data structure
- Kernel refuses delegation without verification method
- Cost tracking per delegation
- Pre-flight assessment in subagent prompt template

### Phase 2: Intelligent Routing
- Task characteristic assessment
- Routing table (taxonomy → model + autonomy + verification)
- Outcome logging for calibration

### Phase 3: Delegatee Pushback
- Challenge protocol (Clarify/Reject responses)
- Orchestrator handles non-compliance gracefully
- Pushback quality tracking

### Phase 4: Learned Delegation
- Route based on historical outcomes
- Dynamic trust calibration from delegatee track records
- Adaptive span of control

---

## 7. Relationship to Other Fawx Docs

| Document | Role |
|---|---|
| `ENGINEERING.md` | Development doctrine — the "contract terms" for code work |
| `DOCTRINE.md` | Kernel-enforced runtime invariants — immutable delegation rules |
| `TASTE.md` | Loadable preferences — tunable delegation parameters |
| `docs/architecture/security-model.html` | Threat model — informs permission handling and trust boundaries |
| This document | Maps formal delegation theory to Fawx's architecture |

---

*This spec captures the design intent. Implementation follows engine stabilization and testing.*
