# H2.4 Issue Context: Model-Aware Prompt Tuning (#558)

Date captured: 2026-02-22
Source type: repository-stable excerpt for spec review reproducibility
Related roadmap anchor: `docs/specs/citros-architecture-roadmap.md` section 2.10

## Problem Statement

H2.4 requires prompt construction behavior that is deterministic and testable across prompt modes (`FULL`, `MINIMAL`, `NONE`), model tiers (`FLAGSHIP`, `STANDARD`, `SMALL`), and accessibility capability state (attached vs detached).

## Required Outcomes

1. Define explicit prompt policy matrix and invariant rules.
2. Quantify prompt budget thresholds and deterministic trim order.
3. Preserve safety guarantees while allowing tier-specific verbosity.
4. Standardize runtime telemetry format for machine parsing and auditing.
5. Define rollout gates and measurable rollback thresholds.
6. Define executable tests that verify all normative constraints.

## Constraints

1. Scope remains H2.4 prompt tuning only (no new tool system redesign).
2. Contracts must be implementation-safe and independently reproducible by reviewers and CI.
3. Safety and confirmation semantics cannot be weakened on smaller models.
