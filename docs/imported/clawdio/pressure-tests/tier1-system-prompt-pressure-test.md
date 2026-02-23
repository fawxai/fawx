# Retroactive Pressure Test: System Prompt Architecture

*Pressure test for #478 — Tier 1 retroactive audit*
*Citros: `PhoneAgentPrompts` | OpenClaw: `system-prompt.ts` + `AgentSession._rebuildSystemPrompt()`*

---

## 1. OpenClaw's Architecture (Source-Level)

### System Prompt Assembly

OpenClaw's system prompt is built from two layers:

**Layer 1: `buildSystemPrompt()` in `system-prompt.ts`** (pi-coding-agent)

A pure function that assembles the prompt from inputs:

```typescript
buildSystemPrompt({
  cwd,
  skills,                  // Pre-loaded Skill[]
  contextFiles,            // Pre-loaded context files (AGENTS.md, etc.)
  customPrompt,            // Replaces entire default prompt
  appendSystemPrompt,      // Appends to end
  selectedTools,           // Which tools to list
})
```

**Assembly order (default prompt):**
1. Identity + role description (hardcoded)
2. Available tools list (dynamically generated from `selectedTools` + `toolDescriptions` map)
3. Custom tools note: "you may have access to other custom tools"
4. Guidelines (dynamically generated based on which tools are active)
5. Pi documentation paths
6. `appendSystemPrompt` (from gateway config — persona, channel info, etc.)
7. Project context files (AGENTS.md, SOUL.md, etc. — each as `## {path}\n\n{content}`)
8. Skills section (formatted by `formatSkillsForPrompt()`)
9. Date/time + working directory (always last)

**Assembly order (custom prompt):**
1. Custom prompt text (replaces everything)
2. `appendSystemPrompt`
3. Project context files
4. Skills section
5. Date/time + working directory

**Key design decisions:**

1. **Tool-aware guidelines**: Guidelines are generated dynamically based on `selectedTools`. If `grep` is available, the guideline says "Prefer grep/find/ls tools over bash". If only `bash` is available, it says "Use bash for file operations." This prevents guidelines from referencing unavailable tools.

2. **Custom prompt can replace entire default**: `customPrompt` bypasses the identity/tools/guidelines sections entirely. This enables completely different personas/agents using the same infrastructure.

3. **Context files injected by the builder, not hardcoded**: AGENTS.md, SOUL.md, USER.md etc. are loaded by `ResourceLoader` and passed as `contextFiles[]`. The prompt builder doesn't know what files exist — it just renders whatever it's given.

4. **Skills are appended after context files**: Skills are formatted separately with their own `formatSkillsForPrompt()` function, which lists them with descriptions and locations so the model can lazy-load them.

5. **Date/time is always last**: Ensures the model sees the current time regardless of how long the prompt is.

**Layer 2: `AgentSession._rebuildSystemPrompt()`** (application layer)

Gathers inputs from various sources and calls `buildSystemPrompt()`:

```typescript
private _rebuildSystemPrompt(toolNames: string[]): string {
  const loaderSystemPrompt = this._resourceLoader.getSystemPrompt();
  const loaderAppendSystemPrompt = this._resourceLoader.getAppendSystemPrompt();
  const appendSystemPrompt = loaderAppendSystemPrompt.join("\n\n");
  const loadedSkills = this._resourceLoader.getSkills().skills;
  const loadedContextFiles = this._resourceLoader.getAgentsFiles().agentsFiles;

  return buildSystemPrompt({
    cwd: this._cwd,
    skills: loadedSkills,
    contextFiles: loadedContextFiles,
    customPrompt: loaderSystemPrompt,
    appendSystemPrompt,
    selectedTools: validToolNames,
  });
}
```

**Rebuild triggers:**
- Session start
- Tool set changes (tool gating, skill activation)
- Model changes (some models have different tool sets)

---

## 2. Citros's Architecture

### `PhoneAgentPrompts` — Single object, ~220 lines

**Assembly order:**
1. Identity (hardcoded `SECTION_IDENTITY`)
2. Tools by category (hardcoded `SECTION_TOOLS`, conditional on `phoneControlAvailable`)
3. Strategy (hardcoded `SECTION_STRATEGY`)
4. Recovery (hardcoded `SECTION_RECOVERY`)
5. Disambiguation (hardcoded `SECTION_DISAMBIGUATION`)
6. Rules (hardcoded `SECTION_RULES`)
7. Runtime (dynamic: model name, accessibility status, timestamp)

**Two prompt types:**
- `buildSystemPrompt()` — full prompt for first turn
- `buildActionPrompt()` — shorter "reminders" prompt for continuation turns

---

## 3. Comparison

### 3.1 Section Modularity

| Aspect | OpenClaw | Citros | Assessment |
|--------|----------|--------|------------|
| Section count | 8-9 (identity, tools, guidelines, docs, append, context files, skills, date/time) | 7 (identity, tools, strategy, recovery, disambiguation, rules, runtime) | **Comparable** |
| Dynamic sections | Tools list, guidelines, context files, skills — all computed at runtime | Only runtime section is dynamic; tools section is static markdown | **Gap**: Citros tools are hardcoded in the prompt. Adding/removing a tool requires code change |
| Custom prompt override | `customPrompt` replaces entire default | No equivalent | **Intentional**: Phone agent has one persona |
| Append mechanism | `appendSystemPrompt` for gateway/channel additions | No equivalent | **Gap — deferred**: When Citros adds gateway-style configuration |
| Context files | Dynamic injection of AGENTS.md, SOUL.md etc. | None | **Intentional**: Phone agent is self-contained, no workspace files |

### 3.2 Tool-Prompt Coupling

| Aspect | OpenClaw | Citros |
|--------|----------|--------|
| Tool list in prompt | Generated from `selectedTools` + `toolDescriptions` map | Hardcoded markdown in `SECTION_TOOLS` |
| Tool descriptions | Map: `{read: "Read file contents", ...}` | Inline markdown: `- tap(element_id) — tap by numeric ID` |
| Adding a tool | Add to `toolDescriptions` map + register tool | Edit `SECTION_TOOLS` markdown + add tool implementation |
| Removing a tool | Remove from `selectedTools` | Remove from markdown (or leave stale) |
| Guidelines match tools | Guidelines dynamically change based on available tools | Strategy section is static regardless of tool set |

**Assessment**: OpenClaw's approach prevents prompt-tool desync — if a tool is gated, its description and guidelines automatically disappear from the prompt. Citros's hardcoded approach means the prompt always lists all 27 tools even if some are unavailable. For the current phone agent (single tool set), this is fine. It becomes a problem when:
- Model-tier-based tool gating is added (H2) — small models shouldn't see API tools
- User-configurable tool sets are added

**Gap — deferred to H2**: Tool descriptions in the prompt should be generated from the registered tool set, not hardcoded.

### 3.3 Prompt Rebuild on State Changes

| Trigger | OpenClaw | Citros |
|---------|----------|--------|
| Tool set changes | Rebuilds system prompt with new tool names | `buildSystemPrompt(phoneControlAvailable=false)` hides entire tools section |
| Model changes | Can change tools based on model capabilities | No model-aware prompt adaptation |
| Skill activation | Rebuilds with new skill entries | N/A (no skills) |
| Session changes | Rebuilds from scratch | N/A |

**Assessment**: Citros has binary tool presence (all or nothing based on accessibility). OpenClaw has granular per-tool control. For H2 (API tools + model tier gating), Citros will need more granular prompt adaptation.

### 3.4 Prompt Content Quality

| Aspect | OpenClaw | Citros | Assessment |
|--------|----------|--------|------------|
| Task-specific guidance | Generic coding guidelines | Phone-specific strategy (direct commands vs tasks, recovery, disambiguation) | **Citros is better here** — domain-specific prompts outperform generic ones |
| Error recovery | None in prompt (relies on model's training) | Detailed recovery section with specific failure patterns | **Citros is better** — phone UI actions need explicit recovery guidance |
| Action prompt | N/A (no separate continuation prompt) | Concise reminders for continuation turns | **Good pattern** — reduces token usage on continuation turns |

### 3.5 Architecture Patterns

| Pattern | OpenClaw | Citros |
|---------|----------|--------|
| Prompt immutability | System prompt rebuilt and SET once per trigger | System prompt built fresh on each call to `buildSystemPrompt()` |
| Separation of concerns | ResourceLoader gathers inputs → builder assembles → agent consumes | Single object does both content and assembly |
| Extensibility | Extension hooks for `session_before_compact`, custom prompts | None — closed system |
| Model-aware prompts | `customPrompt` can vary by model via configuration | No model-specific prompt adaptation |

---

## 4. Gaps Found

### Deferred (file as issues)

1. **Tool descriptions hardcoded in prompt** (H2)
   - When API tools ship with model-tier gating, the prompt must dynamically include/exclude tool descriptions
   - Current approach: editing `SECTION_TOOLS` markdown
   - Target approach: generate tool list from registered tools + gating policy
   - Similar to OpenClaw's `toolDescriptions` map pattern

2. **No model-aware prompt adaptation** (H2)
   - Different models may need different prompt styles (Opus vs Sonnet vs smaller models)
   - OpenClaw handles this via `customPrompt` in configuration
   - Citros should consider prompt variants per model tier (at minimum: detailed for large models, compressed for small)

3. **No prompt rebuild on tool set changes** (H2)
   - When tool gating becomes granular (not just all-or-nothing), the prompt needs to reflect which tools are actually available
   - The `phoneControlAvailable` boolean is insufficient for per-tool gating

### Intentional Divergences

4. **No context file injection**: Phone agent is self-contained — no AGENTS.md, SOUL.md equivalent. The app IS the workspace.

5. **No custom prompt override**: Single-purpose phone agent doesn't need configurable personas.

6. **No skills system**: All tools are built-in Kotlin. WASM skills (H3) will need this.

7. **Continuation prompt (`buildActionPrompt`)**: OpenClaw doesn't have a separate shorter prompt for continuation turns. This is a good optimization for the phone agent where context window is more constrained and tool loops are longer.

---

## 5. Recommendations

### H2 (API Tools)

**Generate tool descriptions from tool registry:**

```kotlin
object PhoneAgentPrompts {
    fun buildToolsSection(
        availableTools: Set<String>,
        toolDescriptions: Map<String, String> = TOOL_DESCRIPTIONS
    ): String {
        // Group by category, filter to available tools
        return TOOL_CATEGORIES
            .map { (category, tools) ->
                val available = tools.filter { it in availableTools }
                if (available.isEmpty()) null
                else "### $category\n" + available.joinToString("\n") { "- ${toolDescriptions[it]}" }
            }
            .filterNotNull()
            .joinToString("\n\n")
    }
}
```

### H2 (Model Tier)

**Model-aware prompt compression:**

```kotlin
fun buildSystemPrompt(
    phoneControlAvailable: Boolean = true,
    modelName: String? = null,
    modelTier: ModelTier = ModelTier.LARGE,
    availableTools: Set<String>? = null
): String {
    // Large models: full prompt with strategy, recovery, disambiguation
    // Small models: compressed prompt with just identity, tools, rules
}
```

### H3 (WASM Skills)

**Skill description injection** similar to OpenClaw's `formatSkillsForPrompt()`.

---

*Pressure test completed 2026-02-16*
*Reference: pi-coding-agent `system-prompt.ts` (190 lines), `AgentSession._rebuildSystemPrompt()` in `agent-session.ts`*
*Citros: `PhoneAgentPrompts.kt` (~220 lines)*
