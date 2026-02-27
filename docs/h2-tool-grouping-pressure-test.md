# Pressure Test: Tool Grouping (#557)

**Issue:** [#557](https://github.com/abbudjoe/fawx/issues/557)
**Date:** 2026-02-21
**Reference Implementation:** OpenClaw v0.x (reverse-engineered from minified dist)

---

## 1. Reference Implementation (OpenClaw)

### 1.1 Skill Discovery & Loading

OpenClaw uses a **skill-based architecture** where tools are organized into skills, each with a `SKILL.md` file containing frontmatter metadata:

```
skills/
  weather/
    SKILL.md       # frontmatter: name, description, requires, os, always
  github/
    SKILL.md
  discord/
    SKILL.md
```

**Discovery pipeline** (`skills-4bbC8Aee.js`, `@mariozechner/pi-coding-agent/skills.js`):

1. **Multi-source loading:** Skills are loaded from 5 sources in priority order:
   - Bundled skills (`openclaw-bundled`)
   - Managed skills (installed via `openclaw skills install`)
   - Personal agent skills (`~/.openclaw/agents/<id>/skills/`)
   - Project agent skills (`.openclaw/skills/` in project dir)
   - Workspace skills
   
2. **Merge by name:** Later sources override earlier ones via `Map.set()` — workspace skills win over bundled.

3. **Eligibility filtering** (`shouldIncludeSkill`): Each skill is evaluated against:
   - `enabled: false` in config → excluded
   - Bundled allowlist (`skills.allowBundled`) → excluded if not on list
   - OS compatibility (`metadata.os` vs `resolveRuntimePlatform()`) — also checks remote node platforms
   - `always: true` flag → always included regardless of requires
   - Runtime requires evaluation: checks binary availability (`hasBinary`), env vars, config paths
   - Remote node binary availability for node-forwarded skills

4. **Invocation policy** (`resolveSkillInvocationPolicy`):
   - `user-invocable: true/false` — whether `/skill:name` commands work
   - `disable-model-invocation: true/false` — if true, excluded from prompt (command-only)

### 1.2 Tool Schema Injection (~97 chars per skill)

Skills are **NOT injected as full tool schemas**. Instead, OpenClaw uses a **two-tier approach**:

**Tier 1 — Compact skill listing in system prompt:**
```xml
<available_skills>
  <skill>
    <name>weather</name>
    <description>Get current weather and forecasts via wttr.in...</description>
    <location>~/.openclaw/skills/weather/SKILL.md</location>
  </skill>
</available_skills>
```

Each skill entry is roughly ~100-200 chars (name + description + path). The system prompt instructs:
> "Before replying: scan `<available_skills>` `<description>` entries. If exactly one skill clearly applies: read its SKILL.md at `<location>` with `read`, then follow it."

**Tier 2 — On-demand SKILL.md reading:**
The model uses the `read` tool to load the full SKILL.md only when it determines a skill is relevant. The SKILL.md contains detailed instructions, when-to-use/when-not-to-use guidance, and tool invocation patterns.

**Prompt budget control** (`applySkillsPromptLimits`):
- `maxSkillsInPrompt` — hard cap on number of skills listed
- `maxSkillsPromptChars` — hard cap on total prompt chars for skills section
- Binary search to find max skills that fit within char budget
- Truncation warning emitted if skills are cut

### 1.3 Tool Count Per Turn — ALL Tools Sent

OpenClaw sends **all registered tools every turn**. The tool list is assembled once at session start in `resolveCommandsSystemPromptBundle()` and includes:

- Core tools: `read`, `edit`, `write`, `exec`, `process`, `browser`, `canvas`, `nodes`, etc.
- Channel tools: `message`, `tts`, etc.
- Plugin tools: loaded via `createOpenClawTools()` and `listChannelAgentTools()`
- Sandboxed variants: `createSandboxedEditTool`, `createSandboxedWriteTool` when in sandbox mode

The tool list in the system prompt is a **summary** (~1 line per tool with description), not full JSON schemas:
```
Tool availability (filtered by policy):
- read: Read file contents
- edit: Make precise edits to files
- exec: Run shell commands
...
```

Full JSON tool schemas are sent via the API's `tools` parameter (standard OpenAI/Anthropic format).

### 1.4 Tool Policy Pipeline (Gating)

**Multi-layer filtering** (`buildDefaultToolPolicyPipelineSteps` + `applyToolPolicyPipeline`):

Tools pass through 7 policy layers in order:
1. **Profile policy** (`tools.profile`) — named tool presets
2. **Provider profile policy** (`tools.byProvider.profile`) — per-model-provider presets
3. **Global policy** (`tools.allow`) — global allow/deny list
4. **Global provider policy** (`tools.byProvider.allow`) — per-provider allow/deny
5. **Agent policy** (`agents.<id>.tools.allow`) — per-agent allow/deny
6. **Agent provider policy** (`agents.<id>.tools.byProvider.allow`) — per-agent per-provider
7. **Group policy** — group-specific restrictions

Plus two additional layers for sandbox/subagent:
- **Sandbox policy** (`sandbox tools.allow`)
- **Subagent policy** (`subagent tools.allow`)

Each layer can have `allow` (allowlist) or `deny` (denylist) semantics. Plugin-only allowlists are stripped to prevent accidentally blocking core tools.

### 1.5 Dangerous Tools Gating

**Static deny lists** (`dangerous-tools-D_CMBLgP.js`):

```javascript
// Gateway HTTP surface — denied by default
DEFAULT_GATEWAY_HTTP_TOOL_DENY = [
  "sessions_spawn", "sessions_send", "gateway", "whatsapp_login"
];

// ACP (Agent Control Protocol) — always require explicit approval
DANGEROUS_ACP_TOOLS = new Set([
  "exec", "spawn", "shell", "sessions_spawn", "sessions_send",
  "gateway", "fs_write", "fs_delete", "fs_move", "apply_patch"
]);
```

**Security scanner** (`skill-scanner-BaJbYPjm.js`):
- Scans skill source code for dangerous patterns before loading
- Rules: `dangerous-exec` (child_process), `dynamic-code-execution` (eval), `crypto-mining`, `suspicious-network`, `potential-exfiltration`, `obfuscated-code`, `env-harvesting`
- Critical findings can block skill loading

### 1.6 Subagent Prompt Differences

Subagents get `promptMode = "minimal"`:
- Skills section **entirely omitted** (no `<available_skills>` block)
- Memory section omitted
- Docs section omitted
- Self-update section omitted
- Model aliases omitted
- User identity section omitted
- Reply tags section reduced
- Voice section reduced
- Extra system prompt labeled as "## Subagent Context" instead of "## Group Chat Context"
- `promptMode = "none"` returns just: `"You are a personal assistant running inside OpenClaw."`

---

## 2. Fawx Current Design

### 2.1 Current State
- **31 tools** across `PhoneTools.kt` (28 in `ALL` + 3 in `API_TOOLS`), all sent every turn
- ~3-4K tokens for tool schemas per turn
- **Dual-model architecture:** `chatClient` (Sonnet) + `actionClient` (Haiku) with different tool access
- `getToolsForModel()` filters API tools (web_search, web_fetch, web_browse) for `ModelTier.SMALL` — security floor prevents untrusted web content on weak models
- `ModelClassifier` with 3 tiers: FLAGSHIP (opus, o1, o3, gpt-5), STANDARD (sonnet, gpt-4o), SMALL (mini, haiku)
- `ActionVerifier` with `VerificationMode` for per-action confirmation — natural extension point for dangerous tool gating
- No skill/plugin architecture — tools are hardcoded in `PhoneTools.kt`
- No on-demand loading — all tool descriptions always present
- No multi-layer policy pipeline — single tier-based filter

### 2.2 Planned Design (#557)
- Group tools into logical categories
- Send relevant groups per turn based on context
- Reduce per-turn token overhead

---

## 3. Comparison

| Dimension | OpenClaw | Fawx Current | Fawx #557 Plan |
|---|---|---|---|
| **Discovery** | Plugin-based, filesystem scan, frontmatter metadata | Hardcoded in `PhoneAgentApi.kt` | TBD |
| **Prompt injection** | Compact XML listing (~100-200 chars/skill) + on-demand `read` | Full JSON schemas every turn (~3-4K tokens) | Group-based subset |
| **Tool count/turn** | All tools sent (schemas), but skills are lazy-loaded | All 27 tools every turn | Subset per group |
| **Policy layers** | 7+ configurable layers (profile, provider, agent, group) | Binary per-model filter | TBD |
| **Dangerous tool gating** | Static deny lists + source code scanning | `ActionVerifier` with `VerificationMode` (per-action confirmation) | Soft gating via dangerous tools list + ActionVerifier |
| **Extensibility** | Filesystem plugins, hot-reload, multi-source merge | Code changes required | TBD |
| **Subagent optimization** | `minimal` prompt mode strips skills, memory, docs | Dual-model with tool filtering, same prompt text | Formal prompt modes per session type |

---

## 4. Gaps

### 4.1 Critical (Must Address Before Implementation)

1. **No on-demand tool description loading.** OpenClaw's key insight is separating the *listing* (compact, always present) from the *instructions* (loaded on demand via `read`). Fawx sends full schemas every turn. Even with grouping, if you send full schemas for a group, you're still burning tokens unnecessarily. **Recommendation:** Consider a two-tier approach where tool summaries are always present but detailed parameter schemas are loaded on demand or deferred.

2. **No tool policy pipeline.** OpenClaw has 7+ configurable policy layers that can restrict tools per provider, per agent, per group, and per session type. Fawx has a single `getToolsForModel()` binary filter. Tool grouping without policy layering means you can't restrict tools for safety contexts (e.g., group chats, automated sessions). **Recommendation:** Design at minimum a 3-layer policy: global → model-tier → context (main/subagent/automation).

3. **Soft dangerous tool gating.** OpenClaw explicitly hard-gates `exec`, `fs_write`, `fs_delete` for ACP surfaces. Fawx has `ActionVerifier` with `VerificationMode` (NEVER/ALWAYS/etc.) which provides per-action confirmation — a natural extension point for soft gating. **Recommendation:** Define a dangerous tools list (e.g., `write_file` to system paths, `open_app` on sensitive apps) and route through `ActionVerifier` with user confirmation prompts. Soft gating (confirm before execute) fits the single-user phone context better than hard deny lists.

### 4.2 Deferred (File as Issues)

4. **No plugin/skill architecture.** OpenClaw's filesystem-based skill discovery enables third-party extensions without code changes. Fawx's hardcoded tools require app releases for new capabilities. This is fine for a mobile app MVP but should be planned for H3+.

5. **No skill source scanning.** OpenClaw scans skill code for dangerous patterns (eval, exfiltration, crypto mining). Not relevant until Fawx has a plugin system, but should be designed alongside it.

6. **No prompt budget control.** OpenClaw binary-searches to fit skills within a char budget. Fawx should have a similar mechanism to prevent tool descriptions from consuming too much of the context window, especially on smaller models.

### 4.3 Intentional Divergences

7. **All tools every turn vs. grouped subsets.** Fawx's plan to send *subsets* per turn based on context is actually more aggressive optimization than OpenClaw, which sends all tool schemas every turn. This is a valid design choice for a mobile app with tighter token budgets, but carries risk: if the model needs a tool that wasn't included in the current group, it can't use it. **Mitigation:** Include a "request more tools" meta-tool or always include a core set.

8. **No filesystem-based skills.** Mobile app can't have filesystem plugins. The grouping mechanism in code serves the same purpose. This is intentional and correct for the platform.

---

## 5. Recommendations

1. **Two-tier tool presentation:** Send compact tool summaries (name + 1-line description) always. Send full parameter schemas only for the active tool group. This mirrors OpenClaw's skill listing + on-demand SKILL.md pattern.

2. **Core + contextual tool sets:** Always include a core set (message, basic tools) and dynamically add contextual groups based on conversation analysis. Include a `request_tools` meta-tool so the model can ask for tools not in the current set.

3. **Policy layering:** Implement at minimum: `global_policy → model_tier_policy → context_policy`. This gives you OpenClaw's most important safety property without the full 7-layer pipeline.

4. **Subagent prompt stripping:** When spawning sub-agents, strip tool groups and prompt sections that aren't relevant to the delegated task. OpenClaw's `minimal` mode is a good template.

5. **Token budget tracking:** Track per-turn token usage for tool schemas. Set a budget (e.g., 2K tokens) and enforce it with truncation + warning, similar to OpenClaw's `applySkillsPromptLimits`.
