# Review Doctrine v2

Review exists to protect four things:

1. **Spec compliance** — the PR solves the intended problem.
2. **Runtime correctness** — the change does not damage turn semantics, tool ordering, replay, persistence, or safety behavior.
3. **Architecture integrity** — the code follows doctrine instead of accumulating central dispatch, hidden contracts, and ad hoc logic.
4. **Long-term velocity** — the codebase gets simpler or sharper, not harder to extend.

Review is not a vibe check. It is a correctness and architecture gate.

---

## Core Principles

### 1. Review is spec-first
Every review starts from the issue and the spec, not from the diff alone.

The first question is:

> Did this PR solve the right problem in the right way?

Not:

> Does this diff seem reasonable?

### 2. Review is independent
The reviewer does not fix the code being reviewed.

Review and fixing are separate roles. The reviewer judges. The fixer fixes. Independence matters.

### 3. Review is binary at the gate
**APPROVE means every section is empty.**

If there is anything in:
- Blocking Issues
- Non-blocking Issues
- Nice-to-haves

then the verdict is:

**REQUEST_CHANGES**

No exceptions. No “approve with notes.” No “good enough.” No “acknowledged but fine.”

### 4. Review protects runtime contracts, not just style
For Fawx, many of the most serious regressions are contract failures:

- wrong turn completion behavior
- tool-use / tool-result ordering corruption
- poisoned history and replay
- hidden tool surfaces
- runtime control-plane drift
- accidental broadening of authority or capability

Review must actively look for these.

### 5. Findings should be separated by type
- **Blocking**: correctness, regression risk, contract breakage, architecture violation, safety violation
- **Non-blocking**: cleanup, simplification, missed reuse, clarity improvements
- **Nice-to-have**: optional polish only

### 6. New bugs should be split out explicitly
If review uncovers a distinct bug or debt pocket that is not really part of the current issue, the reviewer should recommend a follow-up issue rather than blurring boundaries.

Do not split out issues introduced or worsened by the current PR just to avoid blocking it.

### 7. No default deferrals
We do not quietly leave known review findings unfixed unless Joe explicitly approves the deferral.

---

## Review Model

Review now has **two stages**:

1. **Triage Review**
2. **Authority Review**

Both are expected for important PRs.

---

## Stage 1: Triage Review

### Goal
Surface problems early across three lenses before the final merge-gate review.

This stage is advisory, not authoritative.

It does **not** produce APPROVE. It produces findings.

### Inputs
Every triage review must read:

- the issue
- the relevant spec
- `ENGINEERING.md`
- `docs/doctrine.md`
- the full diff

### Output
The triage review must produce findings under three lenses:

1. **Contract / Correctness**
2. **Architecture / Reuse**
3. **Efficiency / Hot Path**

It may also recommend issue splits.

### Lens 1: Contract / Correctness
Questions:

- Does the PR satisfy the issue and spec?
- Does it solve the right problem?
- Does it introduce runtime contract risk?
- Does it change turn completion semantics?
- Does it change tool ordering, replay, or persistence behavior?
- Does it broaden scope beyond the intended fix?
- Are regression tests present and specific enough?
- Are edge cases covered?

This is the highest-priority lens.

### Lens 2: Architecture / Reuse
Questions:

- Did the change reuse existing helpers or abstractions where appropriate?
- Did it introduce duplicated logic?
- Did it add parameter sprawl instead of introducing a struct?
- Did it create a new central dispatcher or string match?
- Did it leak hidden contracts across boundaries?
- Does it violate doctrine: components describe themselves through traits and contracts?

### Lens 3: Efficiency / Hot Path
Questions:

- Did the PR add unnecessary work?
- Did it add repeated file reads or repeated persistence writes?
- Did it add per-turn or startup hot path bloat?
- Did it add regrouping or replay churn?
- Did it add no-op updates or redundant state?
- Did it operate broadly where a narrow operation would do?

### Triage rules
- Triage reviewers do not modify code.
- Triage reviewers do not post an approval verdict.
- Triage findings should prefer concrete evidence over preference.
- Triage may be performed by one reviewer with three explicit sections or by multiple specialized reviewers aggregated into one result.

---

## Stage 2: Authority Review

### Goal
Produce the final merge-gate review.

This is the authoritative review.

### Inputs
The authority reviewer must read:

- the issue
- the spec
- `ENGINEERING.md`
- `docs/doctrine.md`
- the full diff
- triage findings, if present

### Required output format

#### Verdict: [APPROVE or REQUEST_CHANGES]

#### Spec Compliance
- Met / Partially Met / Not Met

#### Blocking Issues
[or "None"]

#### Non-blocking Issues
[or "None"]

#### Nice-to-haves
[or "None"]

#### Follow-up Issues to Split
[or "None"]

#### Summary

### Verdict rule
**APPROVE means all sections are empty.**

If any section has any item, the verdict must be:

**REQUEST_CHANGES**

### Authority review rules
- The reviewer does not write code.
- The reviewer does not soften the verdict because the PR is close.
- The reviewer does not treat “already known” as “safe to merge.”
- The reviewer does not excuse architecture debt if it is introduced by the PR.
- The reviewer may explicitly say a problem belongs in a follow-up issue, but only if that problem is not introduced or worsened by the PR under review.

---

## Fawx-Specific Mandatory Checks

Every authority review must explicitly consider:

### 1. Spec compliance
- Does the implementation match the spec?
- Is scope too broad or too narrow?
- Did the PR solve the real root problem?

### 2. Runtime contract integrity
- Could this affect turn completion?
- Could this affect continuation behavior?
- Could this affect tool ordering?
- Could this affect persisted transcript structure?
- Could this affect replay or provider continuation?

### 3. Tool contract visibility
- Did this change hide a contract behind wrappers or strings?
- Did it reduce kernel visibility into tools or skills?
- Did it add guessed behavior where typed behavior should exist?

### 4. Architecture doctrine
- Does the code follow “everything describes itself”?
- Is behavior owned by the component that should own it?
- Did the change introduce central dispatch or name-based behavior lookup?

### 5. ENGINEERING.md standards
- no `.unwrap()` outside tests
- no functions over 40 lines
- no more than 5 params without a struct
- regression tests for every bug fix
- no band-aids
- root cause thinking

### 6. Diff hygiene
- unrelated changes bundled?
- speculative cleanup unrelated to the issue?
- docs or roadmap churn mixed into runtime fixes?

### 7. Hot-path cost
- any new work on startup?
- any new work per turn?
- any repeated persistence or replay overhead?
- any repeated scans or full-file reads where narrower work is possible?

---

## When to Split a Follow-up Issue

Create or recommend a separate issue when a finding is:

- real
- distinct from the current issue’s intent
- too large to safely fold into the current PR
- a separate architectural seam
- a separate regression with a different chain of custody

Do **not** split out:
- issues introduced by the current PR just to avoid blocking it
- correctness problems that must be fixed before merge

---

## Reviewer Roles

### Triage Reviewer
- advisory
- multi-lens
- non-mutating
- does not approve

### Authority Reviewer
- merge gate
- binary verdict
- non-mutating
- spec-first judgment

### Fixer
- implements changes
- addresses review findings
- is not the reviewer

---

## Recommended Workflow

1. Implementer completes scoped PR against a spec.
2. Triage review runs across:
   - Contract / Correctness
   - Architecture / Reuse
   - Efficiency / Hot Path
3. Findings are aggregated.
4. Fixer addresses findings.
5. Authority review runs.
6. If any section has items: REQUEST_CHANGES.
7. If all sections are empty: APPROVE.
8. Real smoke test still required before merge.

---

## Review Philosophy

We do not review for polish first.

We review for:
1. correctness
2. runtime integrity
3. architecture ownership
4. maintainability
5. efficiency

A PR that is tidy but damages runtime contracts is a bad PR.

A PR that passes tests but corrupts replay semantics is a bad PR.

A PR that solves the immediate bug by deepening hidden contracts is a bad PR.

The point of review is not to bless momentum.

The point of review is to stop avoidable damage.
