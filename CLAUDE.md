# CLAUDE.md - Nova Project

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
- 🔄 **If Joe requests changes after `@abbudjoe ready for merge`** → Full review cycle restarts (push fixes → `@claude review this PR` → address all → repeat until clean → ping Joe again)

## Checklist Before PR

1. ✅ All tests pass (`cargo test`)
2. ✅ No clippy warnings (`cargo clippy -- -D warnings`)
3. ✅ Code formatted (`cargo fmt --check`)
4. ✅ New tests for new features
5. ✅ Documentation updated if needed
6. ✅ `@claude review this PR` comment posted

## Parallel Agent Swarm Rules

When running multiple sub-agents in parallel on the same repo:

1. **ALWAYS use separate git clones** — never share a working directory
   ```bash
   git clone nova nova-agent1
   git clone nova nova-agent2
   cd nova-agent1 && git remote set-url origin https://github.com/abbudjoe/nova.git
   ```
2. **Each agent gets its own directory** — e.g., `nova-agent1/`, `nova-agent2/`, `nova-agent3/`
3. **Each agent works on a separate branch** in its own clone
4. **Agents must NEVER cd into another agent's directory**
5. **Destroy clones after PRs merge** — run `rm -rf nova-agent1 nova-agent2 nova-agent3` once work is merged to main
6. **The primary `nova/` directory stays on `main`** — only clone directories have feature branches

**Why:** Parallel agents sharing one `.git` directory will switch branches on each other, causing total work loss. This was learned the hard way on 2026-02-08.
