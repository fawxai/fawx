# Pressure Test: Model-Aware Prompt Tuning (#558)

**Issue:** [#558](https://github.com/abbudjoe/fawx/issues/558)
**Date:** 2026-02-21
**Reference Implementation:** OpenClaw v0.x (reverse-engineered from minified dist)

---

## 1. Reference Implementation (OpenClaw)

### 1.1 Prompt Mode System

OpenClaw varies system prompts via a **`promptMode`** parameter with three levels:

| Mode | When Used | Effect |
|---|---|---|
| `"full"` | Main agent sessions (direct chat) | All sections included |
| `"minimal"` | Subagent sessions, cron jobs | Major sections stripped |
| `"none"` | Ultra-minimal contexts | Returns single line: `"You are a personal assistant running inside OpenClaw."` |

**Detection** (`reply-B4B0jUCM.js:50806`):
```javascript
const promptMode = isSubagentSessionKey(params.sessionKey) 
  || isCronSessionKey(params.sessionKey) 
  ? "minimal" : "full";
```

This is **session-type aware**, not model-aware. The prompt mode is determined by whether the session is a subagent or cron job, not by which model is being used.

### 1.2 System Prompt Assembly — Conditional Sections

The system prompt is assembled in `buildAgentSystemPrompt()` (~200 lines). Here's what's conditional:

**Always included (all modes):**
- Identity line: `"You are a personal assistant running inside OpenClaw."`
- Tooling section: tool availability list, tool call style guidance
- Safety section: no-independent-goals, human oversight, no manipulation
- OpenClaw CLI quick reference
- Workspace directory and guidance
- Time section (if timezone configured)
- Workspace files / Project Context
- Reasoning format (if reasoning tags enabled)
- Reaction guidance (if configured)

**Included only in `full` mode (stripped in `minimal`):**
- `## Skills (mandatory)` — `<available_skills>` XML block + instructions
- `## Memory Recall` — memory search/get instructions
- `## Docs` — documentation path guidance
- `## OpenClaw Self-Update` — update/config instructions
- `## Model Aliases` — alias table
- `## User Identity` — owner phone numbers
- Messaging section (extended guidance)
- Voice section (extended guidance)
- Reply tags section (extended)

**Conditional on context (independent of mode):**
- `## Group Chat Context` / `## Subagent Context` — `extraSystemPrompt` with different header based on mode
- `## Sandbox` — only if sandbox is enabled
- Reasoning hint — only if `reasoningTagHint` is true
- Owner line — only if owner numbers configured
- Heartbeat prompt — always present but just a single line

### 1.3 Model-Specific Behaviors

OpenClaw has **limited model-specific prompt variation**. The prompt text itself doesn't change based on model. Instead:

1. **Tool parameter normalization** (`normalizeToolParameters`): Tool schemas are adjusted per `modelProvider` (e.g., OpenAI vs Anthropic parameter format differences).

2. **Vision capability** (`modelHasVision`): The `image` tool description changes:
   - With vision: "Analyze one or more images with a vision model. Only use when images were NOT already provided in the user's message."
   - Without vision: "Analyze one or more images with the configured image model (agents.defaults.imageModel)."

3. **Apply-patch model allowlist** (`isApplyPatchAllowedForModel`): The `apply_patch` tool is only included for specific models that handle it well.

4. **Thinking levels** (`thinkingLevel`): Validated against provider-specific thinking level sets (`formatThinkingLevels(provider, model)`).

5. **Provider tool policy** (`tools.byProvider`): Config can restrict tools per provider/model combination.

6. **Model fallback chain** (`runWithModelFallback`): If primary model fails, falls back through configured alternatives — each fallback run gets the same prompt but potentially different tool schemas.

### 1.4 Subagent Prompt Differences

Subagents receive:
- `promptMode = "minimal"` → stripped sections as described above
- `extraSystemPrompt` containing the task description, injected under `## Subagent Context`
- Same tool schemas (filtered by subagent tool policy layer)
- Same core prompt structure (identity, safety, tooling, workspace)

Key insight: **subagents get the same prompt *skeleton* but with ~40-50% of sections removed.** The task-specific context is injected via `extraSystemPrompt`, not by modifying the base prompt.

### 1.5 Runtime Info in Prompt

The prompt includes runtime metadata:
```
Runtime: agent=main | host=clawdio | repo=... | os=Linux... | node=v22 | 
model=anthropic/claude-opus-4-6 | default_model=openai-codex/gpt-5.3-codex | 
shell=bash | channel=telegram | capabilities=inlineButtons | thinking=low
```

This tells the model what it's running as, but the model doesn't use this to self-adjust its behavior — it's informational context.

### 1.6 What OpenClaw Does NOT Do

- ❌ Does not vary prompt verbosity based on model capability (e.g., shorter prompts for smaller models)
- ❌ Does not adjust instruction complexity for weaker models
- ❌ Does not change safety sections per model
- ❌ Does not use different persona/tone per model
- ❌ Does not reduce tool count based on model (tool *policy* can, but it's config-driven, not automatic)

---

## 2. Fawx Current Design

### 2.1 Current State
- `buildSystemPrompt()` in `PhoneAgentPrompts.kt` (398 lines, ~2-3K tokens output)
- **Dual-model architecture already exists:** `chatClient` (Sonnet for user-facing conversation/planning) + `actionClient` (Haiku for action loop iterations). This is effectively session-type prompting — the action model processes untrusted screen content with different tool access.
- `getToolsForModel()` filters API tools (web_search, web_fetch, web_browse) for SMALL tier — `ModelTier.SMALL` is excluded from API tools due to prompt injection risk with untrusted web content
- `ModelClassifier` categorizes models into FLAGSHIP/STANDARD/SMALL tiers with security floor enforcement
- `ActionVerifier` provides per-action verification with configurable `VerificationMode`
- Same system prompt text for all model tiers (no conditional sections)
- No prompt mode enum (no formal full/minimal distinction)

### 2.2 Planned Design (#558)
- Model-aware prompt construction
- Vary prompt content/verbosity by model tier
- Potentially different instruction styles for different capability levels

---

## 3. Comparison

| Dimension | OpenClaw | Fawx Current | Fawx #558 Plan |
|---|---|---|---|
| **Prompt modes** | 3 modes (full/minimal/none) by session type | 1 mode for all | Model-tier based |
| **Model-specific prompts** | Minimal — tool schemas vary, prompt text doesn't | None (except tool filtering) | Planned |
| **Section conditionality** | ~8 sections conditional on mode + context | None | TBD |
| **Subagent optimization** | `minimal` mode strips ~40-50% of prompt | Dual-model (chat/action) with tool filtering, but same prompt text | Extend to formal prompt modes |
| **Tool schema adaptation** | Per-provider normalization, vision-aware descriptions | Per-model tool filtering | TBD |
| **Runtime context** | Full runtime metadata line in prompt | None | TBD |
| **Prompt size** | ~4-6K chars full, ~2K minimal, ~50 chars none | ~2-3K fixed | TBD |

---

## 4. Gaps

### 4.1 Critical (Must Address Before Implementation)

1. **Formalize existing session-type prompting.** Fawx already has a dual-model architecture (`chatClient` for conversations, `actionClient` for phone actions) with different tool access per tier. This is session-type prompting in practice. However, it lacks a formal `PromptMode` enum and the prompt text itself doesn't vary between chat and action contexts. **Recommendation:** Formalize the existing split into `PromptMode.FULL` (chat) and `PromptMode.MINIMAL` (action loop), and strip irrelevant sections (memory instructions, personality, verbose tool descriptions) from action loop prompts. Extend to future delegation modes.

2. **No conditional section architecture.** Fawx has a monolithic `buildSystemPrompt()`. To support model-aware prompting, the prompt must be decomposed into independently toggleable sections. **Recommendation:** Refactor `buildSystemPrompt()` into a section-based builder:
   ```kotlin
   fun buildSystemPrompt(config: PromptConfig): String {
     return listOfNotNull(
       identitySection(),
       toolingSection(config.tools),
       safetySection(),
       if (config.mode != MINIMAL) skillsSection() else null,
       if (config.mode != MINIMAL) memorySection() else null,
       workspaceSection(config.workspace),
       contextSection(config.extraContext),
     ).joinToString("\n\n")
   }
   ```

3. **No runtime context line.** The model doesn't know what model it is, what channel it's on, or what capabilities are available. This is cheap to add and high-value for model self-awareness. **Recommendation:** Add a runtime info line: `Runtime: model=<id> | channel=<channel> | capabilities=<list>`.

### 4.2 Deferred (File as Issues)

4. **Model-specific instruction tuning.** OpenClaw deliberately avoids this — same instructions regardless of model. If Fawx wants to go further (e.g., simpler instructions for Haiku), it should be done carefully with A/B testing. Risk: maintaining N prompt variants is expensive and error-prone.

5. **Prompt budget enforcement.** As prompt sections grow, smaller models may hit context limits. Implement a total prompt char budget with priority-based truncation (safety > tooling > skills > memory > docs).

6. **Provider-specific tool schema normalization.** OpenClaw adjusts tool parameter formats per provider (OpenAI vs Anthropic). Fawx should handle this when supporting multiple providers.

### 4.3 Intentional Divergences

7. **Model-tier prompting (Fawx) vs. session-type prompting (OpenClaw).** Fawx already has implicit session-type prompting via the dual-model chat/action split. OpenClaw doesn't vary prompts by model capability. Both approaches are valid and solve different problems:
   - Session-type: reduces irrelevant context (action loop doesn't need memory/personality)
   - Model-tier: reduces complexity for weaker models
   
   **These are complementary, not competing.** Fawx should formalize the existing session-type split first, then add model-tier as a secondary axis.

8. **No plugin/skill system to strip.** Fawx doesn't have skills, so the biggest win from OpenClaw's `minimal` mode (stripping `<available_skills>`) doesn't apply. But the principle — strip sections that aren't relevant to the current task — absolutely applies to any future feature additions.

---

## 5. Recommendations

1. **Formalize session-type modes.** Fawx's dual-model chat/action split already provides implicit session-type prompting. Create a `PromptMode.FULL` and `PromptMode.MINIMAL` enum to make this explicit, and vary prompt sections based on mode. The action loop doesn't need memory instructions, personality sections, or verbose tool descriptions — strip them for token savings and focus.

2. **Decompose into sections.** Refactor `buildSystemPrompt()` into a section-based builder where each section can be independently included/excluded. This is a prerequisite for both session-type and model-tier tuning.

3. **Add runtime context.** Include a machine-readable line with model ID, channel, and capabilities. Cheap to implement, enables the model to self-adapt.

4. **Model-tier as a secondary axis.** After session-type modes work, add model-tier awareness:
   - **Tier 1 (Opus/large):** Full prompt, all sections, verbose instructions
   - **Tier 2 (Sonnet/mid):** Full prompt, standard instructions  
   - **Tier 3 (Haiku/small):** Reduced prompt, simplified instructions, fewer tools
   
   Keep safety sections identical across all tiers — never weaken safety for smaller models.

5. **Prompt size telemetry.** Log prompt size (chars and estimated tokens) per model per session type. This data will guide optimization decisions and catch prompt bloat early.

6. **Subagent `extraSystemPrompt` pattern.** The existing `chatClient`/`actionClient` split is a natural extension point. When Fawx adds explicit task delegation, inject task-specific context via a dedicated field (like OpenClaw's `extraSystemPrompt`) rather than modifying the base prompt. This keeps the base prompt stable and testable.
