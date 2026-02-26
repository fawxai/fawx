# H2.3 Issue Context (Issue #557)

This file replaces ephemeral `/tmp`-scoped context references for reproducible review and spec verification.

## Scope summary

1. Introduce runtime tool category selection per turn.
2. Enforce deterministic precedence: security/capability constraints, user settings, resolver output, then policy-bounded fallback.
3. Keep CORE always available.
4. Preserve rollout safety via a feature flag and telemetry.

## Requirement anchors used by spec

1. User-level enable/disable controls for non-core categories.
2. Non-bypassable model-tier and capability constraints.
3. Deterministic behavior from current-turn inputs.
4. Fallback behavior constrained to policy allow-set.
