# CLAUDE.md - Citros Project

## Project Overview
AI-native phone agent. Rust daemon running on a rooted Android phone (Pixel 10 Pro) that perceives, thinks, and acts on behalf of the user. Phone-first development approach: Rust agent daemon + Android NDK cross-compilation + direct device control.

## Tech Stack
- Rust (2021 edition)
- Cargo workspace with 11+ crates
- llama.cpp (local LLM via FFI)
- Claude API (cloud LLM)
- wasmtime (WASM skill runtime)
- redb + ring (encrypted storage)
- tokio (async runtime)

## TDD Requirements (MANDATORY)

1. **RED** — Write failing test first
2. **GREEN** — Minimal code to pass
3. **REFACTOR** — Clean up while green

Every feature needs tests. Every bug fix needs a failing regression test first.

## Code Style

- Follow `rustfmt` defaults
- Use `clippy` with warnings as errors
- Type safety over convenience — no `unwrap()` in library code
- `thiserror` for error types, `anyhow` only in binaries
- Async functions for all I/O (tokio)
- Traits for abstractions (e.g., `PhoneActions`, `LlmProvider`)
- Doc comments on all public items

## PR Review Loop (MANDATORY)

**All changes must follow this flow. No exceptions.**

1. Create feature branch from main
2. Write code + tests (TDD: RED → GREEN → REFACTOR)
3. Push branch, open PR
4. **Spawn an opus subagent to review the PR** ← REQUIRED (see Opus Review Process below)
   ⚠️ **Note:** The `@claude review this PR` GitHub Action is currently broken upstream ([anthropics/claude-code-action#947](https://github.com/anthropics/claude-code-action/issues/947)). When it's fixed, revert to using it instead of opus subagents.
5. **Address ALL review items** (every single one)
6. Push fixes
7. **Spawn another opus review after every push** ← REQUIRED
8. Repeat STEPS 5-7 until all issues are resolved
9. **Verify ALL CI checks pass** (all green on GitHub Actions)
10. If CI fails → fix, push, go to step 4
11. Comment `@abbudjoe ready for merge`

### Opus Review Process (NON-NEGOTIABLE)

Every opus reviewer subagent MUST be spawned with ALL of the following:

1. **The full review standards from this file** — TDD mandatory, every feature needs tests, every bug fix needs a regression test, address ALL items including "nice to have"
2. **Explicit test quality checks** — Do tests actually test production code, or do they duplicate logic inline? Do they use the correct APIs and patterns from the existing test suite?
3. **Correct API context** — Include relevant interface signatures, test patterns (e.g. `ScriptedProviderClient`, `setApiModeWithBackends`), and architectural notes so the reviewer can catch stale API usage in tests
4. **Anti-rubber-stamp directive** — "Do NOT rubber-stamp. Missing tests = BLOCKING. If something looks wrong, say so."
5. **Full checklist** — correctness, test coverage, test quality, thread safety, memory management, architecture, edge cases, documentation

The reviewer enforces the SAME standards the `@claude` GitHub Action would have applied. A review that misses missing tests or uses the wrong verdict is a failed review.

**CRITICAL:**
- ❌ No direct commits to main (except hotfixes approved by Joe)
- ❌ No merges without opus code review
- ❌ No skipping review after ANY push
- ❌ Do NOT skip review items marked "low priority", "nice to have", or "suggestion"
- ✅ Review must be posted as a PR comment via `gh pr comment`
- ✅ **Address EVERY comment** — partial fixes are not acceptable
- ✅ **ALL review items must be resolved** — including suggestions, observations, and "nice to have" items. Every single one. No exceptions.
- ✅ **"Optional enhancements for future consideration"** → Create backlog GitHub issues (don't lose them)
- 🔄 **If Joe requests changes after `@abbudjoe ready for merge`** → Full review cycle restarts (push fixes → opus review → address all → repeat until clean → ping Joe again)
- ✅ **ALL CI checks must pass** before commenting `@abbudjoe ready for merge` — verify all checks are green on GitHub Actions

## Checklist Before PR

1. ✅ All tests pass (Android: `./gradlew :chat:testDebugUnitTest :core:testDebugUnitTest`)
2. ✅ New tests for new features / regression tests for bug fixes
3. ✅ Documentation updated if needed
4. ✅ Opus review spawned with full CLAUDE.md standards (see Opus Review Process above)

## Parallel Agent Swarm Rules

When running multiple sub-agents in parallel on the same repo:

1. **ALWAYS use separate git clones** — never share a working directory
   ```bash
   git clone citros citros-agent1
   git clone citros citros-agent2
   cd citros-agent1 && git remote set-url origin https://github.com/abbudjoe/citros.git
   ```
2. **Each agent gets its own directory** — e.g., `citros-agent1/`, `citros-agent2/`, `citros-agent3/`
3. **Each agent works on a separate branch** in its own clone
4. **Agents must NEVER cd into another agent's directory**
5. **Destroy clones after PRs merge** — run `rm -rf citros-agent1 citros-agent2 citros-agent3` once work is merged to main
6. **The primary `citros/` directory stays on `main`** — only clone directories have feature branches

**Why:** Parallel agents sharing one `.git` directory will switch branches on each other, causing total work loss. This was learned the hard way on 2026-02-08.
