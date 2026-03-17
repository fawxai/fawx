# Pre-Ship Backend Completion Plan

Date: 2026-03-17
Context: Joe is polishing the Swift GUIs directly with Codex on the Mac Mini. In parallel, backend/engine work should focus on the last pieces that make Fawx feel functionally complete everywhere except forge.

## Strategic Recommendation

Prioritize the remaining backend work in this order:

1. **PAT borrowing**
2. **Semantic merge validation**
3. **Fitness backpropagation**

Rationale:
- **PAT borrowing** most directly improves the feeling of a complete distributed system.
- **Semantic merge validation** improves trust in self-improvement and experiment outputs.
- **Fitness backpropagation** is valuable, but it is the most architecture-expansive of the three and the easiest to defer if time gets tight.

## Track A — PAT Borrowing

### Goal
Let workers/subagents safely use GitHub credentials without granting merge authority beyond intended scope.

### Important clarification
The roadmap text currently says **"read-only GitHub access for workers"**, but the security note describes a token that can:
- push branches
- create PRs
- comment

That is **not** read-only.

So the design should split this into two explicit modes:

#### Mode 1: Read-only borrow
- inspect repo state
- read PRs/issues/checks
- no pushes
- no PR creation
- no comments that mutate state

#### Mode 2: Contribution borrow
- push branches
- create PRs
- comment on PRs
- cannot merge protected branches because GitHub branch protection blocks that at the platform level

### Security invariants
When implementing PAT sharing to subagents/workers:
1. Branch protection on `staging` + `main` must require repo-owner review before merge
2. Use a fine-grained PAT scoped to the `fawx` repo only
3. No `administration` permission on the PAT
4. Result: subagents can push branches + create PRs + comment, but cannot merge protected branches without owner review approval
5. `dev` remains open for the automated pipeline

### Recommended scope for tonight
Do **not** try to design the final generalized auth-sharing future.
Implement the smallest shippable version:
- configurable borrowed PAT source for worker/subagent GitHub operations
- explicit mode split between read-only and contribution use
- clear guardrails + documentation
- tests for absence, scope routing, and allowed flows

### Implementation steps
1. Audit current worker/subagent GitHub auth path
2. Define borrow source + boundary model
3. Implement **read-only borrow** first
4. Extend to contribution mode only if plumbing stays clean
5. Add explicit tests for:
   - no token available
   - borrowed token attaches only to allowed flows
   - contribution mode stays constrained by documented GitHub branch protection assumptions

### Deliverable
- PR 1: PAT borrowing foundation
- Optional PR 2: contribution-mode borrow if cleanly separable

---

## Track B — Semantic Merge Validation

### Goal
Before accepting experiment/self-improvement outputs, validate that a candidate patch is semantically compatible with the intended target and does not break obvious invariants.

### Recommended scope for tonight
Build an **MVP validator**, not a grand unified merge intelligence layer.

### MVP target
- validate candidate patch applies cleanly
- run targeted compile/test validation on touched crate/scope
- reject obvious stale/conflicting candidates
- surface clear validation output into the experiment flow

### Implementation steps
1. Read the existing spec and current experiment pipeline hooks
2. Identify the narrow insertion point after patch generation and before acceptance/winner selection
3. Implement minimal semantic validation:
   - applyability
   - touched-scope compile/test check
   - simple stale/conflict detection against current branch head if cheap and reliable
4. Add regression tests

### Deliverable
- One focused PR with a narrow semantic validator
- Avoid building a broad framework if a small insertion point works

---

## Track C — Fitness Backpropagation

### Goal
Let experiment outcomes influence future decomposition/prioritization instead of scores terminating as dead-end output.

### Recommended scope for tonight
Treat this as **signal plumbing only** unless the first two tracks land unusually fast.

### MVP target
- persist score/result metadata in a form decomposition can consume later
- do not attempt a full adaptive planner rewrite tonight
- create the data path first

### Implementation steps
1. Identify where experiment scores become final
2. Define a compact reusable feedback artifact
3. Store it where later decomposition/planning can read it
4. Expose one simple hook for future weighting or prioritization

### Deliverable
- Small PR for score signal persistence if it is truly contained
- Otherwise: spec + stub + explicit next-step note

---

## Operational Plan

### Phase 1
- Implement **PAT borrowing** first
- Highest product payoff
- Strongest fit for “complete distributed product” feeling

### Phase 2
- Implement **semantic merge validation** second
- Improves trust in self-improvement without requiring a huge architecture leap

### Phase 3
- Touch **fitness backpropagation** only if runway remains
- Otherwise capture the exact implementation plan and defer cleanly

---

## Recommended Execution Model

To stay aligned with the normal workflow:

1. Architectural read + decomposition in the main session
2. Spawn a **Codex implementer** for PAT borrowing
3. Run an **independent Opus review**
4. Fix to clean
5. Repeat for semantic merge validation
6. Only then decide whether fitness backpropagation fits tonight

This preserves the standard:
- implement
- review
- fix
- re-review
- merge only when fully clean

---

## Honest Ship Recommendation

For tomorrow ship pressure, target this:

### Must land
- **PAT borrowing**
- **Semantic merge validation MVP**

### Only if runway remains
- **Fitness backpropagation**

This gets the maximum “complete system” gain without wandering too deep into forge-adjacent architecture work on the eve of ship.
