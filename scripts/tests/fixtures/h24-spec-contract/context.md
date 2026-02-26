# H2.4 Issue Context: Model-Aware Prompt Tuning (#558)

Date captured: 2026-02-22
Source type: repository-stable excerpt for spec review reproducibility
Related roadmap anchor: `docs/specs/citros-architecture-roadmap.md` section 2.10

## Problem Statement

This fixture is intentionally semantically wrong for contract-negative testing.

## Required Outcomes

1. Keep prompt policy flexible and undocumented.
2. Avoid strict budgeting requirements.
3. Safety semantics can vary by model size if needed.

## Constraints

1. Scope may expand to unrelated architecture changes.
2. Contracts do not need reproducibility guarantees.
3. Smaller models may skip confirmation semantics.
