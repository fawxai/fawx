# Squad Constitution (No-Slop, Non-Negotiable)

This constitution governs all parallel roadmap execution ("Squad") for Fawx.

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

## Squad Execution Mode (CLI Swarms)

For parallel bugfix swarms and review swarms, these rules are mandatory:

1. **CLI-only orchestration**
   - Run swarm workers as Codex CLI background jobs.
   - Do not mix subagents and CLI workers in the same swarm.

2. **One issue/PR per worktree**
   - Each worker gets a dedicated clean worktree created from `origin/staging`.
   - If a worktree contains unexpected edits, reset or recreate it before launch.

3. **Reasoning profile policy**
   - Fix workers default to `high` reasoning.
   - Reviewer workers default to `xhigh` reasoning.
   - Use `xhigh` for fixers only when issue complexity justifies the latency.

4. **Prompt contract for autonomous workers**
   - Workers must proceed with best assumptions and record them.
   - Workers must not block waiting for interactive clarification unless there is a hard safety ambiguity.

5. **Environment policy**
   - Build/test-capable workers must run in an execution mode that can run Gradle and write git metadata for their worktree.
   - If sandboxing prevents required build/commit operations, rerun worker in a mode that allows completion on the trusted build node.

6. **Monitoring + recovery**
   - Monitor workers on a fixed cadence (default 10 minutes).
   - If no meaningful progress is observed for 15 minutes, kill and relaunch that worker with a tighter prompt.

7. **Finisher responsibility**
   - The orchestrator must collect worker outputs, run/confirm tests as needed, complete commit/push/PR/comment steps, and not assume workers fully finalized git operations.
   - In CLI swarm mode, the orchestrator is the default control-plane actor for GitHub write actions (PR review comments, review-thread replies, readiness comments), unless a run explicitly delegates a specific write step to a worker.

8. **Conflict isolation at scale**
   - Do not run parallel fix workers against the same file set/module unless explicitly planned.
   - If two issues overlap in touched files, serialize them or use a lead-worker merge plan.

9. **Idempotent orchestration**
   - Maintain a launch manifest (issue -> branch -> worktree -> pid -> log path).
   - Relaunches must be restart-safe and must not create duplicate PR branches for the same issue.

## Runbook Binding

The operational procedure for this constitution is defined in:
- `docs/runbooks/squad-v2.md`

Any swarm execution must follow that runbook unless explicitly superseded by a newer runbook PR.

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
