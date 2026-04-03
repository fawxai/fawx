# ENGINEERING.md — Immutable Doctrine (Fawx + Ember)

Effective 2026-02-27. These standards apply to all work in `fawxai/fawx` and `fawxai/ember`.

This file is **immutable doctrine**. These rules are machine-enforceable, non-negotiable, and cannot be weakened by the agent. They are the compiled invariants of the development process.

For judgment-based preferences, style conventions, and evolving design taste, see `TASTE.md`.

---

## 0. Core Doctrine

These principles are the foundation. Every rule below derives from them.

### YAGNI — You Aren't Gonna Need It
Do not build functionality until it is necessary. Speculative code is dead weight that must be tested, maintained, and understood by everyone who touches the codebase. If a reviewer can say "we don't need this yet," the code gets removed.

### DRY — Don't Repeat Yourself
Reuse components, functions, and modules to minimize duplication. If the same logic exists in two places, extract it. If extraction is hard, the abstraction is wrong — fix the abstraction.

### Fail Fast and Loudly
Errors must be caught and reported immediately, not hidden. No silent catches, no swallowed exceptions, no error paths that return default values and hope for the best. If something goes wrong, the system tells you what, where, and why.

### Fix Root Causes, Not Symptoms
Address the underlying issue rather than applying a temporary patch. If a fix requires understanding 3 other workarounds, the fix is wrong — remove the workarounds. If the same bug pattern exists elsewhere, fix all instances, not just the one you found.

### Every Dependency Is a Liability
Minimize external dependencies. Every crate, package, or library you add is code you don't control — it can break, become unmaintained, introduce vulnerabilities, or bloat your binary. New dependencies require explicit justification in the PR description: what it provides, why we can't build it in <100 lines, and what the maintenance/security risk is.

### Measure Before Optimizing
Do not optimize code without evidence that it's a bottleneck. Premature optimization adds complexity for no measurable gain. Profile first, identify the hotspot, then optimize — and include the benchmark results in the PR.

---

## 1. Directory Organization

- Every directory must have a clear, singular purpose.
- No "utils" or "helpers" dumping grounds. If something is shared, it gets a named module that describes what it does.
- Flat is better than nested — but when you nest, each level must justify its existence.
- Tests mirror source structure. `src/foo/Bar.kt` → `test/foo/BarTest.kt`. No exceptions.
- Dead code, unused files, and orphaned configs get removed in the same PR that makes them obsolete. Don't leave cleanup for later.

### Fawx repo structure
```
fawx/
├── engine/crates/   ← Rust shared core (fx-* crates)
├── docs/            ← Architecture, specs, decisions
├── scripts/         ← Build, test, validation scripts
└── .github/         ← CI, templates
```

### Ember repo structure
```
ember/
├── crates/
│   ├── protocol-core/
│   ├── agent-web/
│   ├── agent-tools/
│   ├── agent-eval/
│   └── ember-mcp/
├── docs/
├── scripts/
└── .github/
```

---

## 2. Code Quality — No Slop

### What "no slop" means
- Every function does one thing. If you need a comment to explain what a block does, it should be its own function.
- No copy-paste with slight variations. Extract, parameterize, or use generics.
- No TODO/FIXME/HACK that ships without a linked issue. If it's worth noting, it's worth tracking.
- No dead code paths, unreachable branches, or vestigial parameters kept "just in case."
- No `Any`, `Object`, or stringly-typed APIs where a concrete type or enum exists.
- Error handling is explicit. No silent catches, no swallowed exceptions, no `?.let { } ?: run { }` chains that hide failure modes.

### Naming
- Names describe behavior, not implementation. `validateArtifactEvidence()` not `checkStuff()`.
- Boolean names read as questions: `isAttached`, `hasEvidence`, `shouldRetry`.
- No abbreviations unless universally understood (`url`, `id`, `api`). `ctx` is banned; write `context`.

### Functions
- Max ~40 lines. If longer, decompose.
- Max 4-5 parameters. If more, introduce a config/options struct.
- Pure functions preferred. Side effects isolated and explicit.

### Kotlin-specific
- `data class` for value types. No mutable state in data classes.
- Sealed classes/interfaces for state machines and result types.
- Extension functions for behavior, not as a way to avoid putting methods where they belong.
- Coroutine scope is explicit. No `GlobalScope`. Structured concurrency always.

### Rust-specific
- `clippy` clean with `-D warnings`. No `#[allow(clippy::...)]` without a comment explaining why.
- `?` for propagation, not `.unwrap()` outside of tests.
- Types over comments. If a function returns `Result<String, String>`, that second `String` should be a named error type.
- `pub` only what needs to be public. Default to private.

---

## 3. Elegant Solutions Over Band-Aids

### The rule
When faced with a problem, the correct response is the one that makes the codebase simpler after the fix, not just before the next commit.

### What this means in practice
- If a fix requires understanding 3 other workarounds to make sense, the fix is wrong. Remove the workarounds.
- If adding a feature means bolting onto an abstraction that doesn't fit, refactor the abstraction first.
- If two subsystems have grown tangled, untangle them before adding more features on top.
- A PR that makes the codebase worse structurally gets rejected even if it "works."
- Refactoring is not tech debt payoff — it's part of the feature work. Budget for it. Include it in the PR.

### Band-aid indicators (reject these patterns)
- `if (specialCase) { ... } else if (otherSpecialCase) { ... }` growing without bound
- Wrapper functions that exist only to work around a bad interface
- "Temporary" flags or feature gates that never get cleaned up
- Comments explaining why code does something weird instead of fixing the weird thing
- Duplicated logic across files because "it's easier than refactoring the shared path"

---

## 4. Testing — TDD by Default

### The rule
Tests are not an afterthought. They are the first artifact of any behavior change.

### Process
1. **Write the test first.** Before implementing a feature or fix, write a failing test that describes the expected behavior. If you can't write the test, you don't understand the requirement well enough to write the code.
2. **Make it pass.** Write the minimum code to make the test green.
3. **Refactor.** Clean up the implementation while keeping the test green. The test is your safety net — use it.

### What gets tested
- Every public function or method has at least one test.
- Every bug fix comes with a regression test that would have caught it.
- Every error path is exercised — not just the happy path.
- State machines have transition coverage: every valid transition tested, every invalid transition tested for rejection.
- Integration boundaries (Rust ↔ Kotlin via JNI, network clients, file I/O) have contract tests with mocks/fakes at the boundary.

### Test quality standards
- Tests are independent. No test depends on another test's side effects or execution order.
- Tests are deterministic. No flaky tests. If a test is flaky, fix it or delete it — don't skip it.
- Test names describe the behavior, not the implementation: `stops loop when accessibility service is lost` not `testAccessibilityCheck`.
- One assertion per logical behavior. A test that asserts 8 unrelated things is 8 tests pretending to be one.
- No test helpers that hide what's being tested. Setup code is explicit enough that you can read the test top-to-bottom and understand what it does.

### Test organization
- Unit tests live next to the code they test, mirroring source structure.
- Integration tests get their own directory (`tests/integration/` or equivalent).
- Test fixtures and shared fakes live in a dedicated test utilities module — not scattered across test files.

### Coverage expectations
- New code: 80%+ line coverage as a floor, not a target. Critical paths (state machine transitions, policy evaluation, error handling) need 100%.
- Existing code during restructure: if you touch it, you test it. No moving untested code into the new structure without adding tests.

### What reviewers check
- PR has no behavior change without a corresponding test change.
- Tests fail for the right reason before the fix (red-green-refactor verified, or at minimum, the test is clearly correct for the stated behavior).
- Tests don't just assert "no exception thrown" — they assert the actual expected outcome.
- No `@Ignore`, `#[ignore]`, or `skip` without a linked issue and expiration plan.

---

## 5. Review Standards

Every PR gets reviewed against these criteria:

| Criterion | Question |
|-----------|----------|
| **Clarity** | Can someone new to the codebase understand this PR in one read? |
| **Necessity** | Does every changed line serve the PR's stated purpose? |
| **Simplicity** | Is there a simpler way to achieve the same result? |
| **Completeness** | Are edge cases handled? Are tests covering the new behavior? |
| **Consistency** | Does it follow existing patterns, or does it improve them (with justification)? |
| **No regression** | Does it make any existing code harder to understand or maintain? |

Reviewers must flag:
- **Blocking**: Correctness bugs, security issues, architectural violations, slop patterns.
- **Non-blocking**: Style inconsistencies, minor naming, documentation gaps.
- **Nice-to-have**: Suggestions that improve but aren't required.

Authors must not merge with unresolved blocking issues. No exceptions.

---

## 6. The Architecture Contract

Fawx is a TUI-first agentic engine with pluggable shells. The engine is the authority; shells are replaceable.

1. **Eliminate accumulated complexity** — every module re-earns its place.
2. **Establish clean interfaces** — shell ↔ engine boundary is defined by contracts (UISpec), not convenience.
3. **Set the foundation for the OS transition** — nothing we build now should need to be thrown away when new shells are added.
4. **Kernel is immutable at runtime** — the loop orchestrator, policy engine, and enforcement mechanisms cannot be modified by the agent.
5. **Tools belong in the loadable layer** — not hardcoded in the kernel. The kernel provides the `ToolExecutor` trait; implementations live outside.

Code that doesn't meet the standards above gets rewritten or removed.

### Git Branch Model

```
feature/* → dev → staging → main
```

- **feature branches**: cut from `dev`, PRs target `dev`
- **dev**: integration branch — merge freely after CI + TUI smoke test pass. Multiple features tested together here.
- **staging**: release candidate — maintainer manually promotes `dev → staging` after integration testing passes
- **main**: production releases only — `staging → main` for releases

All three long-lived branches are protected: no force push, no deletion.

---

---

*This file is immutable doctrine. Cite it in PR reviews. Changes require explicit user approval. For evolving preferences and style, see `TASTE.md`.*
