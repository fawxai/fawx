# Skill Lifecycle / Signing Release Fixes

## Purpose
This folder breaks the skill lifecycle/signing cleanup into PR-sized slices so the release-blocking fixes can land one piece at a time without muddling command-surface repair, UI wording, docs, and verification.

This pack is intentionally scoped to release blockers:
- restore a real signing flow
- align CLI/TUI/docs on one canonical local-dev workflow
- clarify built vs installed vs server-loaded semantics
- verify that the final product story is coherent before release

## Execution rules
- One file in this folder = one PR-sized slice
- Run sequentially, not in parallel
- Keep changes tightly scoped to the slice
- Do not redesign the skill trust model or invent new product surface area unless required to fix a proven contradiction
- Prefer code/help alignment over speculative new UX

## Suggested execution order
1. `step1-sign-command-surface.md`
2. `step2-skill-state-semantics.md`
3. `step3-docs-and-help-alignment.md`
4. `step4-end-to-end-lifecycle-verification.md`

## Global release goal
A user should be able to build a local skill, understand its current lifecycle state, and use a real signing command if needed without conflicting instructions from the CLI, TUI, docs, or Swift app.
