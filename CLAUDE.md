# CLAUDE.md - Nova Project

## Project Overview
AI-native phone agent. Rust daemon that perceives, thinks, and acts on behalf of the user. Phase 0.5: Mac Mini pre-PoC validating the cognitive pipeline without a phone.

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
2. Make changes, commit
3. Push branch, open PR
4. **Post PR comment: `@claude review this PR`** ← REQUIRED, DO NOT SKIP
   ⚠️ **Note:** This is a comment on the PR (not the commit message)
5. **Wait 2 minutes, then check PR for review comments**
6. **Address ALL comments from the review** (every single item)
7. Commit fixes, **push**
8. **Post PR comment: `@claude review this PR`** ← REQUIRED AFTER EVERY PUSH
9. **Wait 2 minutes, check for new review comments**
10. Issues remaining? → Go to step 6 (address all comments again)
11. When clean: Comment `@abbudjoe ready for merge`
12. Joe reviews and merges

**CRITICAL:**
- ❌ No direct commits to main (except hotfixes approved by Joe)
- ❌ No merges without Claude Code review
- ❌ No skipping the `@claude review this PR` comment (required after EVERY push)
- ❌ Do NOT put `@claude review this PR` in commit messages
- ❌ Do NOT rely on automatic GitHub Action triggers
- ❌ Do NOT skip review items marked "low priority", "nice to have", or "suggestion"
- ✅ The comment must be **standalone** on the PR (not combined with other text)
- ✅ Comment after **every push** to trigger re-review
- ✅ **Check back after 2 minutes** to view and address all review comments
- ✅ **Address EVERY comment** — partial fixes are not acceptable
- ✅ **ALL review items must be resolved** — including suggestions, observations, and "nice to have" items. Every single one. No exceptions.
- ✅ **"Optional enhancements for future consideration"** → Create backlog GitHub issues (don't lose them)

## Checklist Before PR

1. ✅ All tests pass (`cargo test`)
2. ✅ No clippy warnings (`cargo clippy -- -D warnings`)
3. ✅ Code formatted (`cargo fmt --check`)
4. ✅ New tests for new features
5. ✅ Documentation updated if needed
6. ✅ `@claude review this PR` comment posted
