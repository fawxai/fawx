# AGENTS.md - Citros Agent Playbook

This file defines rules for all coding/review agents working in this repository.

## Squad Constitution (Source of Truth)

For parallel roadmap execution ("Squad"), follow:

- `docs/squad-constitution.md`

That document is authoritative for no-slop enforcement.

## No-Slop (Non-Negotiable)

A PR is not done until all review feedback is resolved:
1. Blocking issues
2. Non-blocking issues / nits
3. Nice-to-haves / suggestions

If anything is deferred, create a backlog GitHub issue and link it in the PR comment.

## Review Loop

After every push:
1. Run another full review pass
2. Fix all findings
3. Repeat until clean

## Ready-for-Merge Gate

Only mark ready when:
- No unresolved blocking issues, nits, or nice-to-haves
- Deferred items are tracked with linked backlog issues
- CI is fully green

## Companion Policies

- `CLAUDE.md` (workflow and review standards)
- `CODEX.md` (Codex worker standards)
