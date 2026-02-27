# TASTE.md — Evolving Design Preferences (Fawx + Ember)

Effective 2026-02-27. This file captures judgment-based preferences, style conventions, and design philosophy that guide how we build — but evolve as we learn.

Unlike `ENGINEERING.md` (immutable doctrine), this file is **loadable taste**. The agent can propose changes through the standard development lifecycle. The principles here shape decisions when the doctrine doesn't prescribe a specific answer.

---

## Philosophy

### Solve Problems, Don't Just Write Code
You are here to provide solutions, not to produce lines of code. A 5-line fix that solves the root cause is worth more than a 500-line feature that treats symptoms. Before writing code, ask: "What problem am I actually solving?"

### KISS — Keep It Simple, Stupid
Complexity kills projects. When choosing between two approaches that both work, pick the simpler one. "Simple" means fewer moving parts, fewer abstractions, fewer things that can break — not fewer lines of code. A 20-line function with clear intent beats a 5-line function that requires 3 layers of abstraction to understand.

### Everything Is a Trade-Off
There is no single "best" solution, only the right balance for your specific context. Performance vs readability. Flexibility vs simplicity. Speed of delivery vs thoroughness. Name the trade-off explicitly in PR descriptions and code comments when the choice isn't obvious.

### Software Is Never Finished
Design for change and future extension. Prefer interfaces over concrete implementations. Prefer composition over inheritance. Prefer small, focused modules that can be replaced independently. But don't over-engineer for hypothetical futures (see YAGNI in doctrine).

### Write Code for Humans First, Computers Second
Code is read 10x more than it is written. Optimize for readability: clear names, obvious flow, minimal cleverness. The compiler doesn't care about your variable names — your future self does.

---

## Style

### Document the "Why," Not Just the "What"
Code shows what it does. Comments and commit messages explain why. Every non-obvious decision should have a sentence explaining the reasoning. Architecture Decision Records (ADRs) capture the big calls. Inline comments capture the small ones.

### Commit Messages Tell a Story
- Format: `type(scope): description` (e.g., `feat(kernel): wire structured tool-calling`)
- The description answers "what changed" in imperative mood
- The body (if needed) answers "why" and "what trade-offs were made"
- Reference issue numbers when applicable

### PR Descriptions Are Documentation
A PR description should be readable by someone who wasn't part of the discussion. Include: what changed, why, how it works, what was considered and rejected, and how to test it.

### Review Comments Follow Structure
```
### Blocking Issues
(correctness bugs, security issues, architectural violations)

### Non-blocking Issues
(style, naming, minor improvements)

### Nice-to-haves
(suggestions that improve but aren't required)

### Verdict
APPROVE / REQUEST_CHANGES / COMMENT
```

---

## Process

### Ship Early, Iterate Often
Get feedback quickly to ensure you're building the right thing. A working prototype that validates the approach is worth more than a polished implementation of the wrong thing. PRs should be small enough to review in one sitting.

### Automate Everything You Can
Automate testing, formatting, linting, and CI checks. If a human has to remember to run something, it will eventually be forgotten. Put it in CI. If CI can't enforce it, put it in a pre-commit hook. If neither works, put it in the review checklist.

### Don't Fall in Love with Your Code
Be prepared to abandon or rewrite your work if a better solution arises. Code is a means to an end, not an end in itself. The best engineers delete more code than they write.

### Ask for Help
Do not waste hours stuck on a problem when context from someone (or something) else can unblock you in minutes. Asking for help is not a weakness — it's efficient.

### Never Stop Learning
Technology changes rapidly. Assumptions from last month may be wrong today. Read, experiment, prototype. Challenge your own patterns — if you've been solving problems the same way for a year, you're probably missing better approaches.

---

## Naming & Convention

### Rust Conventions
- Crate names: `fx-*` (Fawx), `ember-*` (Ember)
- Module names: lowercase, descriptive (`loop_engine`, `model_catalog`, not `le` or `mc`)
- Error types: named, specific (`AuthError`, `BudgetExhausted`, not `Error` or `String`)
- Constants: `SCREAMING_SNAKE_CASE`, grouped by purpose at the top of the module

### Unicode Symbols (TUI)
- `↳` (\u{21b3}) for info/metadata lines
- `✗` (\u{2717}) for errors/warnings
- `·` (\u{00b7}) as separator
- `›` (\u{203a}) for prompt arrows

### Color Palette (TUI)
- Banner/headers: Amber `#FFA500`
- User prompt: Bright gold `#FFCC00`
- Assistant label: Amber `#FFA500`
- Metadata: Dim burnt sienna `#D2700A`
- Errors: Orange-red `#FF4500`

---

## Runtime — Self-Development Preferences

These govern how the Fawx orchestrator decomposes and executes work. The available roles and their permissions are doctrine (see `DOCTRINE.md`). How the orchestrator uses them is taste.

### Task Decomposition
- Default to the simplest composition that fits the change. Don't spawn a full review cycle for a typo fix.
- When in doubt, over-decompose rather than under-decompose. Three small PRs are better than one large PR.
- Independent subtasks should be parallelized. Don't serialize work that has no dependencies.

### Complexity Assessment
- **Trivial** (typo, formatting, one-liner bug): Implementer only. Tests are the review.
- **Standard** (single-feature, <500 lines, clear scope): Implementer → Reviewer → Fixer if needed.
- **Complex** (multi-crate refactor, architectural change, >500 lines): Full cycle with possible parallel implementers. May need multiple review rounds.
- **Critical** (security, kernel changes, doctrine-adjacent): Full cycle + mandatory second reviewer.

### Prompt Templates
- Every worker prompt includes: task description, relevant file paths, and ENGINEERING.md rules.
- Implementer prompts include: acceptance criteria, test expectations, scope boundaries ("do NOT touch X").
- Reviewer prompts include: the diff or branch, specific areas of concern, and what the previous review found (if re-review).
- Fixer prompts include: the review findings verbatim, mapping of each finding to required action, and explicit instruction to not defer any finding.

### Worker Pool Sizing
- Default: 1 worker per task. Parallel workers only for clearly independent subtasks.
- Maximum concurrent workers: governed by kernel budget gate, but prefer 3-4 for manageable orchestration.
- If a worker times out, retry once with longer timeout. If still failing, flag as infra-blocked and continue other work.

### Orchestration Cadence
- On worker completion: orchestrate next stage FIRST, then status update.
- On watchdog/reminder: check orchestration state FIRST, spawn next stage if needed, then respond.
- Don't wait for user acknowledgment between stages unless blocked by a decision or merge gate.

---

*This file is living taste. It evolves as we learn what works. Propose changes through the standard development lifecycle — the agent can update this file, with review.*
