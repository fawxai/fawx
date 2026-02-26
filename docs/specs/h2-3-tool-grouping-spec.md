# H2.3 Tool Grouping Spec (Issue #557)

*Stage:* SPEC
*Date:* 2026-02-22
*Branch:* `spec/h23-tool-grouping`
*Source scope:* local repo docs/specs + repo-tracked issue context `docs/specs/references/h23-issue-557-context.md`

---

## 1. Problem Statement

Issue #557 asks for category-based tool sets with conditional inclusion by context, model tier, and user preference to reduce prompt/tool overhead.

Citros already has foundational grouping primitives in `PhoneTools.kt`, `ToolCategory.kt`, `PhoneAgentPrompts.kt`, and `ToolCategoryTest.kt`. The remaining gap is not category definition; it is orchestration: deciding *which* categories are active each turn and honoring user-level enable/disable settings safely.

---

## 2. Current State (as of 2026-02-22)

### Shipped

1. Explicit category mapping for all tools (`ToolCategory` + `TOOL_CATEGORIES`).
2. Core-always-included semantics via `CORE_TOOL_NAMES`.
3. Model-tier gating (SMALL excludes RESEARCH).
4. `request_tools` meta-tool for in-loop category expansion hints.
5. Dynamic tools section builder support in prompts.
6. Tests validating category coverage and basic grouping behavior.

### Not yet solved (scope of this spec)

1. Runtime category selection in `PhoneAgentApi.getToolsForModel()` is still effectively “all categories”.
2. No persisted user preference to disable categories (issue requirement).
3. No clear policy precedence between user settings, model tier security floor, and runtime context.
4. No rollout safety metrics proving token reduction without task regression.

---

## 3. Goals and Non-Goals

### Goals

1. Dynamically choose active tool categories per turn from:
   1. user message context,
   2. model tier policy,
   3. user settings.
2. Keep a safe, reliable always-on core tool set.
3. Reduce tool payload/prompt footprint on average turns.
4. Preserve current behavior behind a kill switch during rollout.

### Non-Goals

1. Plugin/skill filesystem discovery (OpenClaw-style skill loader) in H2.3.
2. New tools or schema redesign.
3. Full policy engine (H2.7 handles ALLOW/CONFIRM/DENY depth).

### Constitution Mapping

1. Safety and policy precedence first: Section 5.2 keeps security/capability constraints non-bypassable and bounds fallback to `policy_allow_set`.
2. Deterministic behavior: Sections 5.2, 5.3, and 8.1 require deterministic resolution for identical inputs and explicit invariants.
3. Privacy by default: Section 8.6 forbids raw user message content in grouping telemetry.
4. Measurable quality gates: Sections 7.3 and 9 define rollout gates and rollback criteria before broad enablement.

---

## 4. OpenClaw Pattern Comparison (Applied to Citros)

### Pattern A: Two-tier tool context (compact listing + detailed on demand)
- OpenClaw: compact skill listing in prompt, detailed SKILL.md loaded only when needed.
- Citros H2.3 adaptation: keep compact summary of all available tools, but only provide detailed category sections and API `tools` schemas for active groups.
- Why adapted, not copied: Citros does not have filesystem skill plugins; categories are static code constructs.

### Pattern B: Multi-layer policy pipeline
- OpenClaw: profile/provider/agent/group policy stacking.
- Citros H2.3 adaptation: minimal precedence stack:
  1. hard security floor (model tier, capability availability),
  2. user category settings,
  3. runtime context resolver,
  4. core fallback.
- Why minimal: lower complexity suitable for single-user mobile runtime.

### Pattern C: Explicit budget controls
- OpenClaw: prompt char-budget clipping for skill section.
- Citros H2.3 adaptation: add per-turn tool-count/token telemetry and budget threshold alarms before hard clipping logic.

### Pattern D: Failure-safe behavior
- OpenClaw: conservative defaults + explicit filtering.
- Citros H2.3 adaptation: fail closed on non-core categories, fail open for core categories only.

---

## 5. Proposed Design

### 5.1 New Concepts

1. `ToolGroupingPolicy` (pure policy resolver)
   1. Inputs: message text, current model tier, capability flags (accessibility, TinyFish key), user category settings.
   2. Output: `ResolvedToolPlan(activeCategories, toolNames, reasonCodes, estimatedToolCount)`.
   3. `activeCategories` is an ordered list with deterministic ordering semantics (see Section 5.2.1 and 7.2).
2. `UserToolCategorySettings`
   1. Persisted map `{category -> enabled/disabled}` for non-core categories.
   2. Core category cannot be disabled.
3. `ContextCategoryResolver`
   1. Lightweight keyword/intent heuristic for first pass.
   2. Conservative: when uncertain, include additional safe categories rather than starve critical tools.

### 5.2 Policy Precedence (normative)

1. **Policy allow-set construction** (highest, non-bypassable):
   1. Start with all categories.
   2. Apply security/model constraints:
      1. SMALL model excludes RESEARCH.
      2. Accessibility-detached excludes phone-control categories.
   3. Apply capability availability constraints (existing runtime gating remains authoritative).
   4. Apply user settings: remove user-disabled non-core categories.
   5. Force CORE into allow-set even if user attempted to disable it.
2. **Context selection**:
   1. Resolver proposes candidate categories from message/context.
   2. Effective active categories = `resolver_candidates INTERSECT policy_allow_set`.
3. **Mixed granularity semantics** (category + tool-level capability gates):
   1. Category-level policy is computed first (`policy_allow_set`).
   2. Tool-level capability filtering is applied after categories are selected (e.g., missing TinyFish key excludes `web_browse` only).
   3. If tool-level filtering removes only some tools in a category, keep that category active (`partial tool pruning`) and emit reason code(s).
   4. If tool-level filtering removes all non-core tools in an active category, the category may remain listed in `activeCategories`, but `toolNames` must contain only surviving tools.
4. **Fallback safety** (lowest, still policy-bounded):
   1. If effective active set is empty, include CORE.
   2. If CORE-only and action-oriented fallback trigger fires, add fallback candidates in order `NAVIGATION`, `INTERACTION`, `OBSERVATION` **only if each candidate is in `policy_allow_set`**.
   3. Fallback must never add any category outside `policy_allow_set`.
5. **Action-oriented fallback trigger (normative)**:
   1. Trigger is true when either condition holds:
      1. Resolver emits `action_intent = true`, or
      2. message contains imperative verb from: `open`, `tap`, `type`, `send`, `enable`, `disable`, `turn on`, `turn off`, `launch`.
   2. Trigger is false otherwise.
   3. Normalization/matching rules (deterministic):
      1. Normalize message text with Unicode NFKC, then lowercase using `Locale.ROOT`.
      2. Match single-word verbs only on whole-word boundaries (`\bverb\b`).
      3. Match multi-word verbs (`turn on`, `turn off`) after whitespace collapsing to a single space.
      4. Use locale-independent matching only; no stemming/lemmatization in v1.
   4. This rule is deterministic and must not rely on prior turns in v1.
6. **Invariant**:
   1. Final categories are always a subset of `policy_allow_set` and always include CORE.
   2. Final `toolNames` are always a subset of tools implied by final categories after tool-level capability pruning.

### 5.2.1 Policy resolution pseudocode (normative)

```text
allow_set = ALL_CATEGORIES
allow_set -= tier_blocked_categories(model_tier)
allow_set -= capability_blocked_categories(capabilities)
allow_set -= user_disabled_non_core_categories(user_settings)
allow_set += CORE

resolver_candidates = resolver_candidates(message)
active_set = resolver_candidates ∩ allow_set
active_set += CORE
if active_set == {CORE} and action_oriented_trigger(message, resolver_signal):
  for c in [NAVIGATION, INTERACTION, OBSERVATION]:
    if c in allow_set: active_set += c

active_ordered = ordered_categories(active_set)

tools = tools_for_categories(active_ordered)
tools -= capability_blocked_tools(capabilities)   # e.g., remove web_browse only
deny_causes = collect_deny_causes(resolver_candidates, allow_set, capabilities, user_settings)
reasons = derive_reason_codes(allow_set, resolver_candidates, deny_causes, active_ordered, tools)

return ResolvedToolPlan(activeCategories=active_ordered, toolNames=tools, reasonCodes=reasons, estimatedToolCount=tools.size)
```

### 5.2.2 Category ordering (normative)

1. `ResolvedToolPlan.activeCategories` must be ordered in canonical category order:
   1. `CORE`
   2. `NAVIGATION`
   3. `INTERACTION`
   4. `OBSERVATION`
   5. `NOTIFICATION`
   6. `CLIPBOARD`
   7. `MEMORY`
   8. `RESEARCH`
   9. `PLANNING`
2. `activeCategories` must contain unique entries only (no duplicates).
3. Fallback expansion (`NAVIGATION`, `INTERACTION`, `OBSERVATION`) is applied to the active set, then final output ordering is canonicalized by the list above.

### 5.3 Resolver state model (normative)

1. V1 is stateless.
2. Inputs are limited to current-turn message + current model tier + current capability flags + current user settings.
3. Prior-turn category memory is out of scope for v1 and explicitly deferred.
4. Determinism requirement applies to the full input tuple above.

### 5.4 Per-turn Flow

1. Receive user turn.
2. Resolve model tier.
3. Load user category settings.
4. Build `policy_allow_set` (security + capabilities + user settings + forced CORE).
5. Run context resolver.
6. Intersect resolver output with `policy_allow_set`.
7. Apply policy-bounded fallback if active set is CORE-only and action-oriented fallback trigger is true.
8. Build:
   1. API `tools` list from resolved categories.
   2. Prompt tools section with summary(all) + detail(active only).
9. Execute turn; collect telemetry.

### 5.5 Default Category Heuristics (v1)

1. Always: CORE.
2. Add NAVIGATION/INTERACTION/OBSERVATION for device-action verbs (open/tap/type/send/settings/app/camera/etc.).
3. Add NOTIFICATION when notification intents detected.
4. Add CLIPBOARD when copy/paste/clipboard terms detected.
5. Add MEMORY when remember/recall/note/save-for-later semantics detected.
6. Add RESEARCH for factual lookup/news/web-site requests (unless tier/policy blocks).
7. Add PLANNING for explicit planning/strategy requests and complex decomposition prompts.

### 5.6 Backward Compatibility

1. Feature flag: `tool_grouping_v1_enabled`.
2. Off path: existing all-category behavior unchanged.
3. On path: dynamic selection active with runtime logging.
4. Emergency fallback: server/client config to force legacy all-category mode.

---

## 6. Edge Cases and Failure Modes (Pressure Test)

### 6.1 False-negative category selection

- Scenario: user says “book me a flight” but resolver omits RESEARCH.
- Risk: agent cannot use web tools.
- Mitigation:
  1. broaden heuristics for booking/travel/ecommerce vocabulary,
  2. keep `request_tools` available in CORE,
  3. fallback expansion rule: if model emits text indicating missing capability, re-run with expanded categories on next turn.

### 6.2 User disables necessary categories

- Scenario: user disables NAVIGATION then asks to open an app.
- Risk: confusing failures.
- Mitigation:
  1. preflight warning in settings for high-impact categories,
  2. runtime assistant message: category disabled, suggest re-enable,
  3. quick re-enable affordance in settings entry point.

### 6.3 Policy conflicts (security floor vs user preference)

- Scenario: SMALL tier + user enables RESEARCH.
- Risk: expectation mismatch.
- Mitigation: explicit precedence docs and user-visible reason code (“Research tools unavailable on SMALL tier model”).

### 6.4 Context oscillation turn-to-turn

- Scenario: rapidly changing category sets produce unstable behavior.
- Risk: non-deterministic tool availability.
- Mitigation:
  1. keep v1 stateless to preserve deterministic behavior from explicit inputs,
  2. measure oscillation via telemetry (`active_category_churn_rate`) and revisit stateful smoothing only in a future version with explicit state in resolver inputs.

### 6.5 Token reduction regresses success rate

- Scenario: fewer tools lowers completion on broad tasks.
- Risk: higher failure/abandon rates.
- Mitigation:
  1. staged rollout with control cohort,
  2. watch completion and retry metrics,
  3. instant rollback via feature flag.

### 6.6 `request_tools` abuse or noise

- Scenario: model repeatedly calls `request_tools` instead of acting.
- Risk: loop inefficiency.
- Mitigation: cap repeated identical `request_tools` calls per turn and inject guidance after threshold.

---

## 7. Implementation Spec (No Code in This Stage)

### 7.1 Files likely touched in IMPLEMENT stage

1. `android/core/src/main/kotlin/ai/citros/core/PhoneAgentApi.kt`
2. `android/core/src/main/kotlin/ai/citros/core/PhoneAgentPrompts.kt`
3. `android/core/src/main/kotlin/ai/citros/core/PhoneTools.kt` (minimal, if needed)
4. New policy/settings classes under `android/core/src/main/kotlin/ai/citros/core/`
5. Settings integration (chat/UI module) for user category toggles.
6. Tests in `android/core/src/test/kotlin/ai/citros/core/` plus settings tests.

### 7.2 Data contract

- `ResolvedToolPlan`
  - `activeCategories: List<ToolCategory>` (ordered in canonical category order from Section 5.2.2; unique values only)
  - `toolNames: Set<String>`
  - `reasonCodes: List<ReasonCode>` (typed enum values below)
  - `estimatedToolCount: Int`

- ReasonCode (typed enum)
  - `tier_small_blocks_research`
  - `user_disabled_navigation`
  - `user_disabled_interaction`
  - `user_disabled_observation`
  - `user_disabled_notification`
  - `user_disabled_clipboard`
  - `user_disabled_memory`
  - `user_disabled_research`
  - `user_disabled_planning`
  - `capability_missing_tinyfish_blocks_web_browse`
  - `capability_missing_accessibility_blocks_phone_control`
  - `fallback_action_intent`
  - `fallback_empty_candidate_set`
  - `core_forced_required`

- `ReasonCode` compatibility policy
  - New enum values must be additive only in minor releases.
  - Existing enum values must not be renamed or repurposed.
  - Clients must ignore unknown enum values for forward compatibility.
- Reason-code derivation contract (normative)
  - `derive_reason_codes` input must include, at minimum: `resolver_candidates`, final `allow_set`, explicit deny causes from tier/capability/user policy, final `activeCategories`, and post-pruning `toolNames`.
  - If a resolver-requested category is excluded, at least one deny reason code for that exclusion must be emitted.
  - Reason derivation must be deterministic for identical resolver output and policy inputs.

### 7.3 Acceptance criteria

1. Categories are resolved per turn, not statically all-on.
2. User can enable/disable non-core categories via settings.
3. Model-tier constraints remain non-bypassable.
4. Core tools always present.
5. Token/tool footprint and safety outcomes meet Section 9 success criteria gates #1-#5 over the specified evaluation windows (token reduction, completion rate, loop quality, `request_tools` frequency, and Policy-violation counter).

### 7.4 ResolvedToolPlan examples (normative)

1. Example A (SMALL tier blocks RESEARCH; action trigger true)
   1. Inputs:
      1. model tier = SMALL
      2. user settings = all non-core categories enabled
      3. resolver candidates = `{CORE, RESEARCH}`
      4. resolver `action_intent = true`
      5. capabilities = all present
   2. Output:
      1. `activeCategories = [CORE, NAVIGATION, INTERACTION, OBSERVATION]`
      2. `toolNames` include only tools implied by those categories
      3. `reasonCodes` include `tier_small_blocks_research`, `fallback_action_intent`
2. Example B (user disables NAVIGATION; fallback cannot reintroduce blocked category)
   1. Inputs:
      1. model tier = STANDARD
      2. user settings disable NAVIGATION
      3. resolver candidates = `{CORE}`
      4. message = "open settings"
      5. resolver `action_intent = false` (trigger still true via imperative verb)
   2. Output:
      1. `activeCategories = [CORE, INTERACTION, OBSERVATION]`
      2. NAVIGATION absent
      3. `reasonCodes` include `user_disabled_navigation`, `fallback_action_intent`
3. Example C (action trigger false; empty resolver output)
   1. Inputs:
      1. model tier = STANDARD
      2. user settings = all enabled
      3. resolver candidates = `{}`
      4. message = "what did I ask you yesterday?"
      5. resolver `action_intent = false`
   2. Output:
      1. `activeCategories = [CORE]`
      2. `reasonCodes` include `fallback_empty_candidate_set`
4. Example D (tool-level pruning keeps category active)
   1. Inputs:
      1. model tier = STANDARD
      2. user settings = all enabled
      3. resolver candidates = `{CORE, RESEARCH}`
      4. capabilities missing TinyFish key
   2. Output:
      1. `activeCategories` may still include `RESEARCH`
      2. `toolNames` excludes `web_browse`
      3. `reasonCodes` include `capability_missing_tinyfish_blocks_web_browse`
5. Example E (normative JSON serialization shape)
   1. `ResolvedToolPlan` serialized payload:
      ```json
      {
        "activeCategories": ["CORE", "NAVIGATION", "INTERACTION", "OBSERVATION"],
        "toolNames": ["request_tools", "open_app", "tap", "read_screen"],
        "reasonCodes": [
          "tier_small_blocks_research",
          "user_disabled_research",
          "fallback_action_intent"
        ],
        "estimatedToolCount": 4
      }
      ```
   2. Rules:
      1. `activeCategories` order is canonical (Section 5.2.2).
      2. `reasonCodes` contains only stable enum values from Section 7.2.
      3. Unknown future `reasonCodes` must be ignored by clients (forward compatibility).

---

## 8. Test Plan

### 8.0 Required invariants (must be explicitly asserted in tests)

1. `final_categories SUBSET_OF policy_allow_set` for every resolution path.
2. CORE is always present in final categories.
3. Fallback cannot reintroduce a category blocked by security, capability, or user disable.
4. User-enable cannot override security/capability deny.
5. Resolver-only categories never bypass policy filters.

### 8.1 Unit tests

1. Policy precedence matrix (required cases):
   1. SMALL + user enables RESEARCH + resolver requests RESEARCH -> RESEARCH absent.
   2. User disables NAVIGATION + resolver requests NAVIGATION -> NAVIGATION absent.
   3. Accessibility detached + action prompt -> phone-control categories absent.
   4. TinyFish key missing + web request -> `web_browse` absent.
   5. User attempts CORE disable -> CORE still present.
2. Fallback invariants (required cases):
   1. Empty resolver output with trigger false -> CORE only.
   2. Empty resolver output with trigger true -> add fallback categories in order only if each category is allowed by policy.
   3. Action prompt with NAVIGATION blocked -> fallback may add INTERACTION/OBSERVATION only if allowed; never NAVIGATION.
   4. Action prompt with NAVIGATION+INTERACTION blocked -> fallback remains CORE (or CORE+OBSERVATION if allowed by policy).
3. Context resolver classification for representative prompts per category.
4. Determinism: identical inputs produce identical `ResolvedToolPlan` including reason codes.
5. Reason-code completeness: every excluded resolver category has at least one deny reason code.
6. Ambiguous prompt negatives:
   1. `"open weather app"` -> includes action categories, does not require RESEARCH.
   2. `"find weather online"` -> includes RESEARCH when policy allows.

### 8.2 Integration tests

1. `PhoneAgentApi` uses resolved category set to construct tools.
2. Prompt builder detail section matches active categories.
3. `request_tools` can request expansion, but expansion remains bounded by `policy_allow_set`.
4. Legacy mode flag yields pre-H2.3 behavior.
5. End-to-end precedence case: security deny + user allow + resolver request + fallback attempt -> denied category remains absent.
6. Concurrency/thread-safety:
   1. settings mutation during resolution cannot produce policy-bypass result,
   2. per-turn resolver observes an atomic settings snapshot.

### 8.3 Regression tests

1. Existing `ToolCategoryTest` and `PhoneAgentApiTest` behavior preserved where intended.
2. SMALL-tier research exclusion remains intact.
3. No regression in web_browse key gating.
4. No regression where fallback previously added NAVIGATION/INTERACTION when blocked (must now remain blocked).

### 8.4 Manual test matrix

1. App control task (“open settings and turn on wifi”).
2. Research task (“find latest weather in Denver”).
3. Notification task (“read my notifications and reply”).
4. Mixed task (“find restaurant and text result”).
5. Disabled-category scenario with user recovery path.
6. Model switch STANDARD -> SMALL and verify research tool removal reasoning.
7. Explicit fallback guard case: disable NAVIGATION + ask action command; verify fallback does not re-add NAVIGATION.
8. Capability guard case: remove TinyFish key + ask web task; verify no web tools after fallback.
9. Accessibility-detached case: ask phone-control command; verify denial reason and no policy breach.

### 8.5 Success metrics (instrumentation)

1. Average tools sent per turn.
2. Estimated tool-schema tokens per turn.
3. Task completion rate.
4. Tool-loop step count distribution.
5. `request_tools` invocation frequency.
6. Policy-violation counter (`final_not_subset_policy_allow_set`) must stay at exactly 0 per day.

### 8.6 Telemetry privacy constraints (normative)

1. Grouping telemetry must include only category names, reason codes, counts, and model tier/capability flags required for policy debugging.
2. Grouping telemetry must contain no raw user message content.
3. Any sampled debug payloads must redact user-provided free text before persistence or transport.

---

## 9. Rollout Plan

### Phase 0: Dark launch (dev only)

1. Land policy resolver + tests behind flag off.
2. Emit telemetry comparing legacy vs resolved plan without changing runtime behavior.

### Phase 1: Internal opt-in

1. Enable `tool_grouping_v1_enabled` for internal testers only.
2. Monitor for regressions in completion and escalation signals.

### Phase 2: Limited production cohort

1. Enable for a small percentage of users/models.
2. Start at 5% of eligible traffic for 7 days, then 25% for 7 days if gates pass.
3. Daily metric review and automated rollback trigger thresholds.

### Phase 3: Default-on

1. Enable by default once completion parity or better is sustained for 14 consecutive days at >=25% traffic.
2. Keep emergency legacy override for one release cycle.

### Rollback criteria

1. Roll back within 24 hours if 24-hour completion rate drops by >=1.5 percentage points absolute vs control.
2. Roll back within 24 hours if p95 tool-loop step count increases by >=20% vs control for 2 consecutive days.
3. Roll back within 24 hours if `request_tools` frequency per 100 turns rises by >=30% vs control for 2 consecutive days.
4. Roll back immediately if policy-violation counter is >0 on any day.
5. Roll back immediately on 2 or more high-severity user reports (P1 capability-missing incidents) within a rolling 48-hour window.

### Success criteria (promotion gates)

1. At least 15% reduction in average estimated tool-schema tokens per turn vs control over a 7-day window.
2. Completion rate is no worse than -0.5 percentage points absolute vs control over the same 7-day window.
3. p95 tool-loop step count does not increase by more than 10% vs control over the same 7-day window.
4. `request_tools` frequency per 100 turns does not increase by more than 15% vs control over the same 7-day window.
5. Policy-violation counter remains exactly 0 for the full evaluation window.

---

## 10. Risks and Open Questions

1. Should category selection use only user text, or also recent tool history/screen context in v1?
2. Should disabled categories be soft-disabled (confirm to enable per task) instead of hard-off?
3. Should we tighten token reduction target from 15% to 20% after first stable release cycle?

---

## 11. Definition of Done for H2.3

1. Spec approved.
2. IMPLEMENT PR delivers resolver + settings + tests.
3. Rollout shows reduced tool payload with no material regression in success rate.
4. Legacy fallback retained until post-stability window.
