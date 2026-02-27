# H1 PR 4: Model Floor — Pressure Test vs OpenClaw

## OpenClaw's Architecture: 4 Layers of Model Control

### Layer 1: Model Allowlist (`buildAllowedModelSet`)
**What:** Config-driven allowlist of which models users can select.
**Where:** `agents.defaults.models` in config → `buildAllowedModelSet()`
**Enforcement:** At model selection time. User picks a model via `/model` command or inline buttons. If the model isn't in the allowlist, selection is rejected.

Key detail: if no allowlist is configured (`rawAllowlist.length === 0`), ALL catalog models are allowed (`allowAny: true`). The allowlist is opt-in restrictive, not opt-in permissive.

**Fawx equivalent:** We have `chatModelsForProvider()` and `actionModelsForProvider()` — hardcoded lists. No user-configurable allowlist yet. Not needed for single-user phone agent (H1), but will matter when we add the model picker UI.

### Layer 2: Per-Session Model Override Validation
**What:** When a user overrides the model for a session, the override is validated against the allowlist.
**Where:** `resolveStoredModelOverride()` → check against `allowedModelKeys`
**Enforcement:** If a stored override references a model that's no longer in the allowlist (e.g., admin removed it), the override is silently reset to default.

```javascript
if (allowedModelKeys.size > 0 && !allowedModelKeys.has(key)) {
    applyModelOverrideToSessionEntry({
        entry: sessionEntry,
        selection: { provider: defaultProvider, model: defaultModel, isDefault: true }
    });
    resetModelOverride = true;
}
```

**Fawx equivalent:** We have SharedPreferences model selection per-provider. No validation on load — stale model IDs persist (known issue, filed as #391). This is a **gap**.

### Layer 3: Security Audit (Advisory, Not Enforcement)
**What:** Three audit checks warn about model security risks:

1. **`models.legacy`** — Legacy/obsolete models (severity: warn)
   - Pattern matching: `LEGACY_MODEL_PATTERNS` (regex-based)
   - "Older/legacy models can be less robust against prompt injection"

2. **`models.weak_tier`** — Below-recommended-tier models (severity: warn)
   - `isGptModel() && !isGpt5OrHigher()` → "Below GPT-5 family"
   - `isClaudeModel() && !isClaude45OrHigher()` → "Below Claude 4.5"
   - "Smaller/older models are generally more susceptible to prompt injection and tool misuse"

3. **`models.small_params`** — Small parameter models with dangerous tool exposure (severity: **critical** if unsandboxed)
   - `inferParamBFromIdOrName()` → detect models ≤ threshold params
   - Checks if `web_search`, `web_fetch`, `browser` are allowed for that model
   - If small model + web tools + no sandbox → **critical** finding
   - Remediation: sandbox=all AND deny web tools

**Key insight:** OpenClaw does NOT hard-block small models. It **warns** operators and escalates to critical when the combination is dangerous (small model + untrusted input tools). The enforcement is advisory — operators choose whether to act.

**Fawx equivalent:** Our `ModelClassifier` hard-blocks SMALL tier at `PhoneAgentApi` construction time. **We're actually MORE restrictive than OpenClaw** on this. We reject the model entirely; they audit and warn.

### Layer 4: Model Failover Chain
**What:** When an API call fails (rate limit, auth, timeout, billing), automatically rotate to the next model/provider.
**Where:** `FailoverError` class → `resolveFailoverReasonFromError()` → fallback chain
**Reasons:** billing (402), rate_limit (429), auth (401), timeout (408), format (400)

Each auth profile gets a cooldown on failure. The system tries the next profile/model in the chain until one succeeds or all are exhausted.

**Fawx equivalent:** **None.** When our API call fails, the loop catches the exception and returns an error. No retry with different auth, no model fallback. This is a **significant gap for H2/H3** (roadmap §3.1).

### Layer 5: Prompt Mode Adaptation
**What:** System prompt adapts based on context:
- `promptMode: "full"` — full system prompt with all sections (main sessions)
- `promptMode: "minimal"` — trimmed prompt (sub-agents, resource-constrained)
- `promptMode: "none"` — bare minimum ("You are a personal assistant running inside OpenClaw.")

**NOT the same as model-aware prompting:** Prompt mode is based on session type, not model capability. All models get the same prompt mode for a given session type. The H2 "Model-Aware Prompt Tuning" from our roadmap is something OpenClaw does NOT do — they don't adjust prompt complexity based on whether the model is Opus vs Sonnet.

**Fawx equivalent:** We have `buildSystemPrompt()` with conditional sections based on `phoneControlAvailable` and `modelName`, but prompt content doesn't vary by model tier. The H2 roadmap item (different prompts for Opus/Sonnet/Haiku) goes BEYOND what OpenClaw does.

---

## Comparison Matrix

| Concern | OpenClaw | Fawx | Gap? |
|---|---|---|---|
| **Hard model blocking** | No — advisory audit warnings only | Yes — `ModelClassifier` rejects SMALL tier at construction | We're MORE restrictive ✅ |
| **Model allowlist** | Config-driven, validated at selection | Hardcoded lists in `ModelConfig` | Minor — OK for single-user H1 |
| **Model tier classification** | Pattern-based in audit (`isGpt5OrHigher`, `isClaude45OrHigher`) | Pattern-based in `ModelClassifier` (FLAGSHIP/STANDARD/SMALL) | Equivalent ✅ |
| **Unknown model default** | No enforcement (audit only) | Defaults to STANDARD (permissive) | ⚠️ Design difference — see below |
| **Stale model override** | Silently reset to default on load | Persists in SharedPreferences | **Gap** — #391 |
| **Model + tool exposure** | Audit checks tool exposure per model tier | No per-model tool restriction | **Gap for H2** — when API tools arrive |
| **Failover chain** | Full: auth rotation, model fallback, cooldowns | None — single attempt, fail on error | **Gap for H3** — roadmap §3.1 |
| **Prompt mode** | Session-type-based (full/minimal/none) | Accessibility-based (tools section conditional) | Different axis, both valid |
| **Model-aware prompting** | Not implemented | Planned for H2 (per-tier prompt variants) | We're ahead on design |
| **Model validation UI** | `/model` command, inline buttons, allowlist check | Model picker dropdown, no validation | Minor UX gap |

---

## Gaps Found

### Gap 1: Stale Model Override (existing issue #391)
**Problem:** When model IDs change (e.g., Anthropic deprecates a date-stamped ID), users with the old ID in SharedPreferences silently get API errors.
**OpenClaw's approach:** Validates stored overrides against the current allowlist on every session load. Silently resets invalid overrides to default.
**Fix:** On app startup / model selection load, validate stored model ID against `ModelConfig.allKnownModels()`. If invalid, reset to default and show a one-time notice.
**Priority:** Medium — already filed as #391.

### Gap 2: Model + Tool Exposure Analysis (future)
**Problem:** When we add API tools (web_search, http_request), a small model with web tools is a security risk — susceptible to prompt injection from web content.
**OpenClaw's approach:** `collectSmallModelRiskFindings()` checks if small models have `web_search`/`web_fetch`/`browser` access. Critical severity if unsandboxed.
**Fix:** When API tools ship, add a model tier check: SMALL tier models should not have access to web tools. This naturally fits the `wrapToolWithBeforeToolCallHook` pattern or a boundary check.
**Priority:** Deferred to when API tools ship. File now as a note.

### Gap 3: Model Failover (H3)
**Problem:** Single-provider, single-model. API failures are fatal.
**OpenClaw's approach:** Full failover chain with auth profile rotation, model fallback, per-profile cooldowns.
**Fix:** Already in roadmap §3.1. No action now, but note that the failover design should account for model floor — fallback models must also be above floor.
**Priority:** H3. Already tracked.

### Gap 4: Unknown Model Default is Permissive
**Problem:** `ModelClassifier` defaults unknown models to STANDARD (permitted). A new small model we haven't seen could bypass the floor.
**OpenClaw's approach:** Also permissive — their audit doesn't block unknown models.
**Fix:** This is intentional (documented in `ModelClassifier` KDoc: "new models from major providers are typically mid-tier or above; defaulting to SMALL would block legitimate new models"). But worth noting: when model catalogs become dynamic (#391), we should validate against known catalogs rather than pattern matching alone.
**Priority:** Low — acceptable risk for now. Pattern matching catches all current small models.

---

## Verdict: Current Implementation is Solid

The model floor implementation (`ModelClassifier` + `ModelConfig.isModelAboveFloor()` + `PhoneAgentApi` rejection) is actually **more restrictive** than OpenClaw's approach. OpenClaw audits and warns; we hard-block. For a phone agent processing untrusted screen content, hard-blocking is the right call.

**No changes needed for H1.** The gaps are:
1. Stale model override (#391) — already tracked, medium priority
2. Model + tool exposure — deferred to API tools work
3. Failover — H3, already in roadmap

---

*Pressure test completed 2026-02-16*
*Reference: OpenClaw dist — `auth-profiles`, `audit`, `reply`, `model-overrides`, `failover-error`*
