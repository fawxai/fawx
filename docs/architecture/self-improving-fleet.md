# Architecture: Self-Improving Fleet Coordination

## Overview

Before Fawx can run self-improving fleets (multiple nodes autonomously improving the codebase), it needs a coordination layer that prevents chaos. This document defines the minimum viable stack.

The key insight: **git is already a DAG**. Branches are decomposition nodes, commits are refinement chains, PRs are proposals upward, merges are composition. We don't need new infrastructure — we need intelligence on top of what git already provides.

---

## The Hybrid Model

Pure DAGs lack refinement discipline. Pure linear chains lack decomposition. The hybrid combines both:

```
                    ┌─────────────────────┐
                    │   Goal (main/dev)    │
                    │  "improve scoring"   │
                    └──────────┬──────────┘
                               │
                    ┌──────────┴──────────┐
              decompose                decompose
                    │                      │
          ┌────────┴────────┐    ┌────────┴────────┐
          │  Subtask A       │    │  Subtask B       │
          │  "fix evaluator" │    │  "fix chain-fwd" │
          └────────┬────────┘    └────────┬────────┘
                   │                      │
            chain: A1→A2→A3        chain: B1→B2→B3
            (score, reject,        (score, reject,
             iterate)               iterate)
                   │                      │
              winner: A3             winner: B2
                   │                      │
                   └──────────┬───────────┘
                              │
                    ┌─────────┴──────────┐
                    │   Semantic Merge    │
                    │   + Validation      │
                    └────────────────────┘
```

- **DAG layer**: Problem decomposition. Agent or user breaks a goal into independent subproblems.
- **Chain layer**: Iterative refinement at each leaf. Fawx's existing experiment pipeline — score, reject, retry.
- **Merge layer**: Compose winning solutions, validate the composition.

---

## Build Order

Each phase is independently useful and testable. No big-bang.

### Phase 1: Cross-Branch Scoring (Tournament Mode)

**What**: Extend the experiment evaluator to compare results across branches, not just within a single chain.

**Why**: Currently, each chain scores in isolation. "Branch A scored 0.87" and "Branch B scored 0.91" exist as separate facts. There's no mechanism to say "B wins" or "A and B are solving different problems and both should merge."

**How**:
- Add a `tournament` subcommand to the experiment CLI
- Input: list of branch names or chain entry hashes
- Evaluator runs the same test suite against each branch's winning state
- Output: ranked results with scores, plus a compatibility matrix (do A and B touch overlapping files?)
- Store tournament results as a new chain entry type: `tournament_result`

**Depends on**: Existing experiment infrastructure (chain entries, evaluator, scoring)

**Test gate**: Run tournament across 3 branches with known-different scores, verify ranking matches expected order.

### Phase 2: Auto-Decomposition

**What**: Agent analyzes a high-level goal and generates subtask branches with scoped experiment specs.

**Why**: Today, decomposition is manual — Clawdio reads the codebase, identifies subproblems, writes specs. This should be a formal pipeline step.

**How**:
- New command: `fawx experiment decompose --goal "improve test coverage for fx-kernel"`
- Agent analyzes the goal against the codebase (file structure, test coverage, complexity metrics)
- Generates N subtask specs, each with:
  - Scope (files/modules to modify)
  - Signal (what to measure)
  - Hypothesis (expected improvement)
  - Isolation constraint (must not touch files in sibling scopes)
- Creates one branch per subtask from the current HEAD
- Writes experiment config per branch

**Key constraint**: Scope isolation. Sibling subtasks MUST have non-overlapping file scopes. If two subtasks need to touch the same file, they must be sequenced, not parallelized. This is the worktree isolation lesson from PR #896 applied at the experiment level.

**Depends on**: Phase 1 (need tournament scoring to compare results after decomposition)

**Test gate**: Decompose a known multi-file improvement goal, verify scopes don't overlap, verify each subtask spec is runnable.

### Phase 3: Semantic Merge Validation

**What**: After composing winning solutions from sibling branches, validate that the composition is at least as good as the individual winners.

**Why**: Git merge handles textual conflicts. It doesn't catch semantic conflicts — two independently correct changes that break when combined.

**How**:
- After git merge of winners: run the full test suite + experiment evaluator
- Compare merged score against individual winner scores
- Three outcomes:
  - **Clean**: Merged score ≥ max(individual scores) → accept
  - **Degraded**: Merged score < max but > 0 → flag for review, surface which combination caused regression
  - **Broken**: Merged score = 0 or build failure → reject merge, report conflict

**Escalation**: On degraded/broken, the system surfaces the conflict but does NOT auto-resolve. Manual resolution initially. Phase 4 (backpropagation) would eventually automate re-running leaf experiments with awareness of siblings.

**Depends on**: Phase 1 (scoring) + Phase 2 (decomposition creates the branches to merge)

**Test gate**: Intentionally create two branches with overlapping semantic impact (e.g., both optimize the same function differently). Verify merge validation catches the conflict.

### Phase 4: Fitness Backpropagation (Future)

**What**: When a merge fails validation, propagate that signal back to the leaf experiments that contributed, so they can retry with awareness of their siblings' solutions.

**Why**: Without this, failed merges are dead ends. With it, the system can say "your fix works alone but conflicts with sibling X — try again knowing X's approach."

**How**: TBD — this is the research frontier. Options:
- Feed sibling's winning diff into the retried experiment's prompt
- Add a constraint to the experiment spec: "must be compatible with [diff]"
- Run a new chain that starts from the merged state and tries to resolve the conflict

**Depends on**: Phases 1-3 working reliably.

---

## Fleet Implications

Once Phases 1-3 are solid, fleet self-improvement becomes:

1. **Primary node** decomposes goal into subtask branches
2. **Worker nodes** each take a subtask branch and run experiment chains
3. Workers push winning commits to their branches
4. **Primary node** runs tournament scoring across branches
5. Primary merges winners with semantic validation
6. Clean merges get proposed as PRs to dev
7. Failed merges get surfaced for review (or Phase 4 backprop)

The fleet protocol (fx-fleet) already handles node registration, task routing, and communication. The missing piece is this coordination logic — which is all engine-side, no new network protocol needed.

---

## What We're NOT Building

- **New version control** — git is the DAG, period
- **Distributed consensus** — the primary node is the authority
- **Speculative execution** — experiments are deterministic (same input → same score, modulo model variance)
- **Autonomous merge to main** — all merges to staging/main remain human-gated

---

## Relationship to Existing Systems

| System | DAG | Chains | Scoring | Merge | Decomposition |
|--------|-----|--------|---------|-------|---------------|
| Git | ✅ | ✅ (commits) | ❌ | ✅ (textual) | ❌ |
| Fawx experiments (today) | ❌ | ✅ | ✅ | ❌ | ❌ (manual) |
| DVC | ✅ | ❌ | ❌ | ❌ | ❌ |
| W&B / MLflow | Partial | ❌ | ✅ | ❌ | ❌ |
| **Fawx fleet (target)** | **✅** | **✅** | **✅** | **✅** | **✅** |

---

## Open Questions

1. **Decomposition granularity** — how deep should auto-decomposition go? One level? Recursive until subtasks are "atomic"? What defines atomic?
2. **Score comparability** — can you meaningfully compare scores across different scopes? "Evaluator accuracy improved 12%" vs "chain-forward latency improved 8%" — these aren't on the same scale.
3. **Model variance** — same experiment, different model temperature seeds → different scores. How much variance is acceptable before declaring a winner?
4. **Resource budgeting** — fleet experiments cost money. How do you allocate budget across branches? Equal split? Proportional to estimated impact?

---

*This document is the foundation for post-dogfooding work. Each phase gets its own spec when implementation begins.*
