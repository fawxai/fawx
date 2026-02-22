# CODEX.md - Codex Worker Standards

Codex workers in this repository must follow the same quality bar as any other reviewer/implementer.

## Squad Constitution (Source of Truth)

- `docs/squad-constitution.md`

Use it as the canonical policy for no-slop execution.

## Mandatory No-Slop Gate

Do not mark a PR done while any of the following remain unresolved:
- Blocking issues
- Non-blocking issues / nits
- Nice-to-haves / suggestions

Deferrals are allowed only with a backlog issue + PR link.

## Review-After-Every-Push

Every push requires a fresh review pass. Continue fix → review loops until clean.

## TDD + CI

- RED → GREEN → REFACTOR is mandatory
- Missing tests for feature/bug-fix work is blocking
- CI must be green before ready-for-merge

## Related Policy Files

- `CLAUDE.md`
- `AGENTS.md`
- `docs/squad-constitution.md`
