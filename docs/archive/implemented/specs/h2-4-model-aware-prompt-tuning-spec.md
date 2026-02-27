# H2.4 Spec: Model-Aware Prompt Tuning (#558)

**Stage:** SPEC  
**Issue:** #558  
**Issue context source:** `docs/specs/h2-4-issue-558-context.md`  
**Roadmap anchor:** `docs/specs/fawx-architecture-roadmap.md` section 2.10  
**Date:** 2026-02-22

## 1. Scope

Define and harden model-aware prompt tuning behavior so prompt construction is predictable, testable, and safe across:
- prompt modes (`FULL`, `MINIMAL`, `NONE`)
- model tiers (`FLAGSHIP`, `STANDARD`, `SMALL`)
- capability state (accessibility attached vs detached)

This is a SPEC-first deliverable. Production implementation lands in a follow-up PR, but this spec now includes executable contract checks and concrete test IDs so conformance is immediately verifiable.

### 1.1 In Scope (for next implementation PR)

1. Prompt policy matrix as a first-class contract (mode x tier x capability).
2. Prompt budget guardrails with deterministic section priority under token pressure.
3. Tool-description verbosity policy per tier without weakening safety text.
4. Runtime context standards (stable fields, timestamp behavior, redaction rules).
5. Observability requirements for prompt size and section selection.
6. Test coverage requirements for prompt composition and safety invariants.
7. Rollout controls, telemetry gates, and rollback triggers.

### 1.2 Out of Scope

1. New tools, action-policy engine, or memory-system redesign.
2. Provider-specific tool schema translation (future multi-provider hardening item).
3. Sub-agent implementation (only prompt-mode readiness is specified here).
4. UI work beyond exposing telemetry already produced by backend logs.

## 2. Current Baseline (as of 2026-02-22)

**Baseline evidence snapshot commit:** `9af3ce894999`

From current code/docs:
- `PhoneAgentPrompts.buildSystemPrompt(...)` supports modular sections.
- `PromptMode` exists with `FULL`, `MINIMAL`, `NONE`.
- `ModelTier` classification exists and affects prompt detail (`SMALL` gets reduced strategy/tools text).
- Action loop uses minimal prompt path (`buildActionPrompt` / `PromptMode.MINIMAL`).
- Runtime line exists (`model`, `tier`, `accessibility`, UTC timestamp).
- `PromptModeTest` validates section inclusion/exclusion and tier fallback behavior.

Remaining H2.4 value is hardening and operationalization: explicit contracts, budget enforcement, telemetry standards, and rollout safety.

## 3. OpenClaw Pattern Comparison (Relevant Only)

### 3.1 What to Copy

1. Prompt modes as an explicit axis (`full`/`minimal`/`none` concept).
2. Conditional section inclusion as policy, not ad hoc string editing.
3. Runtime metadata injected into prompt for model self-awareness.
4. Treat prompt size as a resource with hard limits and deterministic trimming.

### 3.2 What to Adapt (Fawx-Specific)

1. OpenClaw is session-type first; Fawx needs both session-type and model-tier behavior.
2. OpenClaw skill loading is filesystem/plugin oriented; Fawx is fixed-tool mobile architecture.
3. Fawx must prioritize mobile latency and token cost more aggressively on `SMALL` tier.

### 3.3 What Not to Copy

1. Weakening safety text for smaller models.
2. Adding large plugin/skills complexity into H2 prompt tuning scope.
3. Overfitting to provider-specific quirks in this H2.4 slice.

## 4. Proposed Prompt Policy Contract

### 4.1 Required Invariants

Each invariant has a stable ID for auditability:

- `INV-001`: Security block is present in `FULL` and `MINIMAL` for all tiers.
- `INV-002`: Safety semantics are equivalent across tiers via canonical safety clauses and approved shortening rules only.
- `INV-003`: `NONE` mode is non-agentic only and must not be used for tool-capable turns.
- `INV-004`: Runtime section format is stable and machine-parseable with fixed key order.
- `INV-005`: Tool prompts never include categories disallowed by active model-tier tool policy.
- `INV-006`: Accessibility detached state must include capability warning and remove action-first phone-control guidance.
- `INV-007`: Prompt construction and telemetry emission must be thread-safe under concurrent requests.

### 4.2 Mode x Tier Matrix

| Mode | FLAGSHIP | STANDARD | SMALL |
|---|---|---|---|
| `FULL` | Full strategy, detailed tool guidance, full recovery/comms/rules | Same sections, moderate verbosity | Reduced tool/strategy verbosity, same safety constraints |
| `MINIMAL` | Compact execution reminders + safety | Same | Shortest actionable reminders + same safety |
| `NONE` | Identity only, no tools/safety/runtime | Same | Same |

Additional rule:
- `phoneControlAvailable=false` strips actionable phone-tool guidance and injects accessibility warning in `FULL`/`MINIMAL`.

### 4.3 Prompt Budget Policy

Token estimate method: `estimated_tokens = ceil(utf8_char_count / 4)`.

Normative default budgets (estimated tokens):

| Mode | Soft Budget | Hard Budget |
|---|---:|---:|
| `FULL` | 2200 | 2600 |
| `MINIMAL` | 900 | 1100 |
| `NONE` | 40 | 60 |

Rules:
1. Emit warning telemetry when soft budget is exceeded.
2. Deterministically trim to hard budget using this exact order (lowest priority first):
   1. verbose examples
   2. communication style detail
   3. recovery elaboration
   4. tool parameter detail
   5. strategy detail
3. Never trim these sections:
   1. identity baseline
   2. security block
   3. critical execution rules (`type_text` does not submit, stale-ID warning)
   4. capability warning when accessibility is unavailable
   5. runtime line (except in `NONE`)

Worked examples:
1. Example A (`FULL`, `STANDARD`):
   - Pre-trim size: `2740` estimated tokens (over hard budget by `140`).
   - Trim steps: remove verbose examples (`-95`), then communication style detail (`-52`).
   - Final size: `2593` estimated tokens. Strategy and safety sections remain intact.
2. Example B (`MINIMAL`, `SMALL`):
   - Pre-trim size: `1148` estimated tokens (over hard budget by `48`).
   - Trim steps: remove recovery elaboration (`-31`), then shorten tool parameter detail (`-20`).
   - Final size: `1097` estimated tokens. Security block and critical execution rules remain verbatim.

### 4.4 Canonical Safety Text Contract

Safety invariance is enforced through canonical safety clauses and deterministic normalization rules.

Canonical safety clauses:
- `SAFE-001`: "Never perform irreversible or high-stakes user actions without explicit confirmation."
- `SAFE-002`: "If tool output is ambiguous, stale, or missing required identifiers, request clarification before acting."
- `SAFE-003`: "Do not claim task completion unless the required UI state or tool result confirms completion."
- `SAFE-004`: "When accessibility control is detached, report the limitation and avoid action instructions that require detached capabilities."

Allowed shortening rules (only these):
1. Replace repeated whitespace with a single space.
2. Remove parenthetical clarifiers that do not alter modal verbs (`must`, `must not`, `never`, `do not`).
3. Convert punctuation style (`;` vs `.`) without removing obligations/prohibitions.

Disallowed shortening rules:
1. Removing negations (`not`, `never`, `do not`).
2. Removing confirmation requirements.
3. Removing stale/ambiguous-output safety checks.

Conformance test requirement:
- normalize canonical and emitted safety text using allowed rules and assert equality for all `FULL`/`MINIMAL` tier variants.

### 4.5 Runtime Line Schema

Runtime line schema (single line, pipe-delimited, fixed key order):

`runtime|ts=<RFC3339 UTC>|model=<model_name>|tier=<FLAGSHIP|STANDARD|SMALL>|mode=<FULL|MINIMAL|NONE>|accessibility=<attached|detached>|tool_policy=<policy_id>|prompt_chars=<int>|prompt_tokens_est=<int>|trimmed=<true|false>|trimmed_sections=<comma_list_or_none>`

Normative details:
1. Timestamp format: RFC3339 UTC (example `2026-02-22T18:04:27Z`).
2. Delimiter: literal `|` between fields, `=` within key-value pairs.
3. Key order: exactly as listed in schema; no reordering.
4. Redaction rules:
   - `model` may include provider model ID only.
   - Do not include user content, tool arguments, contact names, or message text in runtime line.
   - `trimmed_sections` may include section IDs only (not section content).
5. `trimmed_sections` must use canonical section IDs sorted lexicographically ascending; use `none` when no sections were trimmed.

## 5. Failure Modes and Guardrails

### 5.1 Failure Modes

1. Prompt bloat causes higher latency/cost or context eviction.
2. Small-tier prompts become too sparse, causing tool misuse loops.
3. Accidental safety regression in a trimmed variant.
4. Wrong mode selected for a tool-enabled turn (`NONE` misuse).
5. Stale runtime data or non-deterministic formatting breaks observability.
6. Capability mismatch: prompt says tools are available when accessibility is off.
7. Drift between prompt policy and tool policy (`getToolsForModel` vs described tools).
8. Concurrency bug causes section leakage between parallel requests.

### 5.2 Guardrails

1. Mode-selection guard:
   - reject `NONE` for any `chatWithTools` call path.
2. Safety-presence and equivalence guard:
   - fail fast in tests if `FULL`/`MINIMAL` prompt lacks canonical safety clauses.
3. Budget guard:
   - emit warning when soft budget exceeded; enforce deterministic hard-cap trimming.
4. Tool-policy guard:
   - generated tool description set must be derived from active tool policy, not static assumptions.
5. Runtime-format guard:
   - enforce fixed schema, fixed key ordering, and RFC3339 UTC timestamp.
6. Accessibility guard:
   - explicit warning text when phone control detached; no contradictory action-first guidance.
7. Concurrency guard:
   - no mutable static/shared prompt builder state; each request uses isolated prompt assembly buffers.

## 6. Test Plan (Implementation Exit Criteria)

### 6.1 Unit Tests

- `UT-H24-001`: full matrix coverage for mode x tier x capability combinations.
- `UT-H24-002`: canonical safety clauses present and equivalent after normalization in `FULL`/`MINIMAL`.
- `UT-H24-003`: over-budget fixtures trigger deterministic trimming order.
- `UT-H24-004`: runtime line parses against schema and fixed key order.
- `UT-H24-005`: tool-enabled paths reject/avoid `NONE`.
- `UT-H24-006`: described tool groups match actual allowed tools for `SMALL` vs others.

### 6.2 Integration Tests

- `IT-H24-001`: `PhoneAgentApi` chat turn (`FULL`) and action loop (`MINIMAL`) prompt selection.
- `IT-H24-002`: accessibility detached flow shows warning and no contradictory tool guidance.
- `IT-H24-003`: tier inference fallback (`modelName` present, tier omitted) remains stable.

### 6.3 Manual / Device Validation

- `MT-H24-001`: compare token usage and latency before/after budget policy.
- `MT-H24-002`: verify action-loop reliability on small-tier models under multi-step tasks.
- `MT-H24-003`: verify ambiguous-contact flows still ask user (high-stakes disambiguation).
- `MT-H24-004`: verify no regression in direct command execution (`open_app`, `press_home`, `press_back`).

### 6.4 Concurrency Tests

- `CT-H24-001`: run prompt builds concurrently across mixed mode/tier/capability inputs; assert no cross-request section contamination.
- `CT-H24-002`: run telemetry emission in parallel; assert one well-formed runtime line per request with no field interleaving.

### 6.5 Invariant-to-Test Mapping

| Invariant | Contract | Required Tests |
|---|---|---|
| INV-001 | Security block present in `FULL`/`MINIMAL` | `UT-H24-001`, `UT-H24-002` |
| INV-002 | Safety semantics equivalent via canonical clauses | `UT-H24-002` |
| INV-003 | `NONE` forbidden in tool-capable turns | `UT-H24-005`, `IT-H24-001` |
| INV-004 | Runtime line schema + key order fixed | `UT-H24-004`, `CT-H24-002` |
| INV-005 | Tool-policy alignment by tier | `UT-H24-006`, `IT-H24-003` |
| INV-006 | Accessibility detached warnings and behavior | `UT-H24-001`, `IT-H24-002` |
| INV-007 | Thread-safety in prompt + telemetry paths | `CT-H24-001`, `CT-H24-002` |

## 7. Rollout Plan

### 7.1 Phase 0: Instrumentation First

1. Add prompt metrics logging:
   - mode, tier, token estimate, included sections, trim flags.
2. Keep behavior unchanged until telemetry confirms baseline.

### 7.2 Phase 1: Guardrails + Budgets Behind Flag

1. Enable policy under feature flag (default off).
2. Internal dogfood with both `STANDARD` and `SMALL` models.
3. Monitor:
   - prompt size distribution
   - tool-loop completion rate
   - user-visible failure rate
   - per-turn latency/cost deltas

### 7.3 Phase 2: Controlled Enablement

1. Enable for `MINIMAL` mode first (highest ROI, lowest blast radius).
2. Expand to `FULL` mode after one stable cycle.
3. Keep one-command rollback path (disable flag, revert to baseline builder behavior).

### 7.4 Rollback Triggers

Immediate rollback if any occur for a rolling 60-minute window:
1. tool-loop failure or abandonment rate increases by `>= 5.0%` absolute vs pre-rollout baseline.
2. any confirmed safety or confirmation-policy regression (`>= 1` verified event).
3. wrong-mode production events (`NONE` used in tool-capable turn) exceeds `0.1%` of tool-capable turns.
4. p95 latency for `SMALL` tier increases by `>= 20%` with no corresponding completion-rate gain (`< 1.0%` absolute).

## 8. Deliverables for Implementation PR

1. Prompt policy contract codified in code comments/docs.
2. Guardrail checks in prompt construction + call-site mode selection.
3. Prompt budget enforcement + logging.
4. Complete test matrix from Section 6.
5. Short operator runbook: enable, monitor, rollback.

## 9. Acceptance Criteria (SPEC Complete)

1. Spec defines scope, constraints, and non-goals for H2.4.
2. OpenClaw comparison is explicit with copy/adapt/do-not-copy decisions.
3. Failure modes are pressure-tested with concrete guardrails.
4. Test plan and rollout plan are explicit and actionable with stable test IDs.
5. Runtime schema, safety contract, budgets, and rollback thresholds are quantified and deterministic.
6. Spec is checked in under `docs/specs/` with executable contract validation script.

## 10. Glossary

- `security block`: Non-trimmable section containing mandatory safety and confirmation constraints (`SAFE-001` to `SAFE-004`).
- `critical execution rules`: Non-trimmable operational rules that prevent UI-action errors (for example, `type_text` does not submit and stale-ID warnings).
- `tool-capable turn`: Any turn where tool invocation is enabled or expected (`chatWithTools` and action-loop execution paths).
- `runtime line`: Single machine-parseable metadata line emitted with each prompt build.
