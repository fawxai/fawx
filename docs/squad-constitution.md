# Squad Constitution (No-Slop, Non-Negotiable)

This constitution governs all parallel roadmap execution ("Squad") for Citros.

## Core Rule: No-Slop Quality Gate

A PR is **not done** until all review feedback is resolved:

1. Blocking issues
2. Non-blocking issues / nits
3. Nice-to-haves / suggestions

There are no optional review items in Squad mode.

## Deferrals (Allowed Only with Traceability)

If an item is intentionally deferred, all of the following are required:

1. Open a backlog GitHub issue for that exact item
2. Link the backlog issue in a PR comment
3. State why it is deferred + when it will be addressed

No silent deferrals.

## Review-After-Every-Push Loop

After **every push** to a PR branch:

1. Run a fresh review pass
2. Address all new findings
3. Repeat until review returns clean

Do not mark a PR ready while unresolved review items exist.

## Ready-for-Merge Criteria

A PR may be marked ready only when all conditions are true:

- No unresolved blocking issues
- No unresolved nits
- No unresolved nice-to-haves/suggestions
- Any deferred item has a linked backlog issue in PR comments
- CI checks are all green

## Enforcement

These rules apply to humans, OpenClaw, Codex workers, Claude workers, and subagents equally.


## 5.2 Policy Precedence

Security constraints are non-bypassable and must be applied before lower-priority policy layers.

## 5.3 Determinism

Given identical explicit inputs, policy decisions must be deterministic.

## 7.3 Acceptance Criteria

Spec updates must define measurable acceptance gates and satisfy them before broad rollout.

## 8.1 Testing Baseline

Behavioral changes require TDD coverage for pass and fail boundaries.

## 8.6 Privacy Baseline

No raw user message content may appear in policy telemetry.

## 9. Rollout And Gates

Rollout requires explicit promotion gates and rollback criteria.
