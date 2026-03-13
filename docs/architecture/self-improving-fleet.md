# Self-Improving Fleet Coordination

Architecture doc for Fawx's distributed self-improvement system. Hybrid DAG decomposition + linear refinement chains.

---

## Core Insight

**Git IS the DAG.** No new version control infrastructure needed.

| Git concept | Fleet concept |
|---|---|
| Branches | Problem decomposition (subtask isolation) |
| Commits | Chain steps (iterative refinement) |
| PRs | Proposals (scored candidates for integration) |
| Merges | Composition (combining validated subtask results) |

The coordination layer is intelligence on top of git, not a replacement for it.

---

## The Hybrid Model

Two complementary strategies operating at different scales:

### DAG: Decomposition Layer
High-level goals are broken into independent subtask branches. Each branch is a scoped experiment context — its own chain, its own scoring, its own workers. Branches can run in parallel when subtasks are independent, sequential when coupled.

**Example:** "Improve error handling across the codebase" →
- Branch A: `fx-kernel` error propagation
- Branch B: `fx-llm` provider error types  
- Branch C: `fx-channel-telegram` error surfacing

Each branch gets scoped experiments with cargo-scoped evaluation (`-p <crate>`).

### Chains: Refinement Layer
Within each branch, linear chains drive iterative improvement. Each step sees prior results and builds on them. The auto-chain mechanism (`--max-rounds`) already handles this — reject + score > 0 triggers another round.

### Merge: Composition Layer
After subtask branches produce validated winners, a merge step composes them. The merge is itself scored — does the composed result maintain or exceed individual branch scores? Failed merges surface for resolution rather than being forced.

---

## Build Order

Each phase is independently useful and testable. No big-bang.

### Phase 1: Cross-Branch Tournament Scoring
Extend the experiment evaluator to compare results across branches, not just within a chain. The scoring infrastructure exists — add a "tournament" mode that evaluates multiple branch winners against the same fitness criteria.

**Prerequisite:** Existing experiment chain + scoring pipeline.
**Delivers:** Objective comparison of competing approaches to the same problem.

### Phase 2: Auto-Decomposition
The agent analyzes a high-level goal, generates subtask branches with scoped experiments. This formalizes what the orchestrator already does manually: goal → analysis → subtask specs → branch-per-spec.

**Prerequisite:** Phase 1 (need to score subtask results).
**Delivers:** Automated problem decomposition without human intervention.

### Phase 3: Semantic Merge Validation
Build-verify is the floor (compiles + tests pass). Add a scoring step after merge: does the composed result score at least as well as the individual winners? If regression detected, flag the conflict.

**Prerequisite:** Phase 2 (need multiple branch results to compose).
**Delivers:** Safe automated integration of parallel improvements.

### Phase 4: Fitness Backpropagation (Future)
Scores from composition flow back to inform decomposition strategy. If certain decomposition patterns consistently produce better merge outcomes, the system learns to prefer them.

**Prerequisite:** Phase 3 + enough data to identify patterns.
**Delivers:** Self-improving decomposition strategy. Can wait for v1.

---

## Fleet Implications

### Worker Model
Each fleet worker runs a scoped experiment on a branch. Workers don't need full project context — they receive:
- A branch name
- A scope (crate/module)
- Chain history (prior results)
- Fitness criteria

The primary holds the DAG, dispatches subtasks, collects results, and manages composition.

### Concurrency
- **Independent subtasks:** Parallel across fleet workers
- **Coupled subtasks:** Sequential on same worker (or ordered dispatch)
- **Chains within a branch:** Sequential (each step depends on prior)

### PAT Borrowing
Workers need read-only GitHub access to fetch branches and evaluate code. PAT borrowing gives them scoped, time-limited access without storing long-lived credentials on worker machines.

---

## Comparison: Current vs Target

| Capability | Current (manual) | Target (automated) |
|---|---|---|
| Decomposition | Clawdio analyzes, writes specs, assigns subagents | Agent generates subtask branches from goal description |
| Refinement | Auto-chain within single experiment | Auto-chain + cross-chain learning |
| Composition | Manual merge + review | Scored merge with regression detection |
| Scheduling | Human triggers experiments | Fleet-aware dispatch based on worker capacity |
| Learning | Chain history within scope | Fitness backprop across decomposition patterns |

---

## What We Learned (PR #896, #1063)

Self-improving without coordination is just multiple agents stomping on each other:
- **PR #896:** Two subagents on the same worktree committed partial state, produced a broken PR
- **PR #1063:** Two parallel reviewer→fixer chains ran 5 rounds each because the replacement orchestrator didn't check existing state

The coordination layer exists specifically to prevent these failure modes at fleet scale.

---

## Open Questions

1. **Decomposition granularity:** How fine-grained should subtasks be? Too coarse = lost parallelism. Too fine = merge overhead dominates.
2. **Merge conflict resolution:** When semantic merge fails, what's the escalation path? Human-in-the-loop? Re-decompose?
3. **Scoring cross-crate:** Current experiments scope to a single crate. Cross-crate changes need a composition-level fitness function.
4. **Worker trust:** Should workers self-evaluate, or should evaluation always happen on a trusted node?

---

*Written 2026-03-13. Foundation for Wave 9+ after migration readiness is complete.*
