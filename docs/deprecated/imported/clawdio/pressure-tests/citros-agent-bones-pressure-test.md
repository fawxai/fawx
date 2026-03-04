# Agent Bones — Pressure Test & Design Doc

## Reference Implementation: OpenClaw

### How OpenClaw Wires Identity Files

**File Discovery & Loading** (`agent-scope.ts`):
- Reads 8 files from workspace dir: AGENTS.md, SOUL.md, TOOLS.md, IDENTITY.md, USER.md, HEARTBEAT.md, BOOTSTRAP.md, MEMORY.md
- All loaded at session start via `loadWorkspaceBootstrapFiles()`
- Files are **injected as-is** into the system prompt under a `# Project Context` header
- Each file becomes a `## /path/to/FILE.md` section with raw content appended
- Missing files are silently skipped (flagged `missing: true`)

**Prompt Architecture** (`pi-embedded.ts`):
- System prompt is assembled from **hardcoded platform sections** + **user-editable workspace files**
- Platform sections come first: tooling, safety, skills, memory recall, messaging, voice, etc.
- Workspace files are appended at the end as "Project Context"
- Special handling: if SOUL.md exists, an instruction is added: "embody its persona and tone"
- The workspace files **supplement** the platform prompt — they never replace it

**Key insight: OpenClaw's workspace files are READ-ONLY context.** The agent doesn't write to SOUL.md or AGENTS.md during normal operation. It reads them at session start, they shape behavior, and only the human (or the agent at the human's request) edits them between sessions. The agent DOES write to MEMORY.md and `memory/*.md` — but that's memory, not identity.

**Bootstrap Lifecycle:**
1. Fresh workspace → BOOTSTRAP.md seeded from template
2. BOOTSTRAP.md tells agent to run onboarding conversation
3. Agent fills in IDENTITY.md, USER.md, SOUL.md based on conversation
4. Agent deletes BOOTSTRAP.md when done → `onboardingCompletedAt` recorded
5. Subsequent sessions: no BOOTSTRAP.md, agent reads identity files directly

**Subagent Filtering:**
- Subagents/cron sessions only get AGENTS.md + TOOLS.md (not SOUL.md, MEMORY.md, etc.)
- Prevents personality leakage and token waste in background tasks

**Memory System:**
- `memory_search` tool: semantic search across MEMORY.md + `memory/*.md`
- `memory_get` tool: read specific lines from memory files
- Agent is instructed to search memory before answering questions about prior work
- MEMORY.md = curated long-term memory (agent maintains)
- `memory/YYYY-MM-DD.md` = daily raw logs
- Memory files are workspace files the agent reads/writes via file tools

### How Fawx Currently Works

**File System** (`AgentFileManager.kt`):
- Manages `agent/` directory: SOUL.md, USER.md, AGENTS.md, SECURITY.md, TOOLS.md, MEMORY.md
- Missing: IDENTITY.md, BOOTSTRAP.md, HEARTBEAT.md
- `read_file`, `write_file`, `list_files` tools exist and work
- `dailyMemoryPath()` exists but nothing calls it
- SECURITY.md is read-only (enforced)

**Prompt Assembly** (`AgentPromptBuilder.kt` + `PhoneAgentPrompts.kt`):
- TWO parallel systems that don't compose:
  - `PhoneAgentPrompts` — hardcoded modular prompt (identity, tools, strategy, recovery, rules, runtime)
  - `AgentPromptBuilder` — reads markdown files and concatenates them
- `OnboardingPersistence.systemPromptForStartup()` picks one:
  - If SOUL.md + USER.md exist → `AgentPromptBuilder.full()` (loses ALL phone tools/strategy!)
  - If not → `PhoneAgentPrompts.buildSystemPrompt()` (loses ALL identity/personality!)
- **This is the core bug**: once onboarding writes SOUL.md, the agent loses its phone control instructions

**Onboarding** (`OnboardingFlow.kt` + `OnboardingPersistence.kt`):
- Scripted conversational flow captures: agent name/nature/vibe/emoji, user name, relationship style, boundaries
- Writes SOUL.md (bare bones: name, nature, vibe, emoji, relationship style)
- Writes USER.md (name, address, preferences, boundaries, context)
- Personality prefs captured but never injected into prompt

**Memory:**
- `remember` tool → SQLite (`MemoryProvider` interface)
- `recall` tool → SQLite FTS5 search
- MEMORY.md file exists in `AgentFileManager` but is never read or written by memory tools
- Two memory systems (SQLite + files) are completely disjoint

---

## Gap Analysis

### Critical (must fix before shipping)

| # | Gap | OpenClaw | Fawx | Impact |
|---|-----|----------|--------|--------|
| 1 | **Prompt composition** | Workspace files supplement platform prompt | Workspace files REPLACE platform prompt | Agent loses phone tools after onboarding |
| 2 | **SOUL.md depth** | Rich persona doc (tone, boundaries, philosophy) | 5-line bullet list (name, vibe, emoji) | Agent has no real personality |
| 3 | **Memory bridge** | MEMORY.md is the primary memory store, searchable | MEMORY.md never written; SQLite is separate | No persistent memory across sessions |

### Important (should fix)

| # | Gap | OpenClaw | Fawx | Impact |
|---|-----|----------|--------|--------|
| 4 | **IDENTITY.md** | Separate from SOUL.md — factual identity (name, creature, avatar) | Missing — identity crammed into SOUL.md | Conflates who-you-are with how-you-behave |
| 5 | **BOOTSTRAP.md lifecycle** | Exists → triggers onboarding → deleted when done | Missing entirely | No self-healing onboarding state |
| 6 | **Daily memory** | `memory/YYYY-MM-DD.md` pattern, agent writes daily logs | `dailyMemoryPath()` exists, never called | No temporal memory structure |
| 7 | **Prompt hot-reload** | Files re-read each session; editing SOUL.md takes effect next session | Writing SOUL.md at runtime doesn't rebuild prompt until wallet mutation | Identity edits don't take effect |

### Deferred (nice to have)

| # | Gap | OpenClaw | Fawx | Impact |
|---|-----|----------|--------|--------|
| 8 | **HEARTBEAT.md** | Drives periodic proactive checks | Missing — no heartbeat concept on phone | No proactive behavior |
| 9 | **Subagent filtering** | Only AGENTS.md + TOOLS.md for background tasks | No subagents on device | N/A for now |
| 10 | **TOOLS.md auto-population** | Agent writes device/tool notes here | Never initialized | Agent can't record tool-specific knowledge |

---

## Design: Where Fawx Should Diverge from OpenClaw

### 1. Prompt Composition — Merge, Don't Replace

OpenClaw appends workspace files as "Project Context" after platform instructions. This works because OpenClaw's platform sections are about tool availability and coding patterns — generic enough that persona files don't conflict.

**Fawx is different.** The phone agent prompt has highly specific sections (tool docs, tap strategy, recovery patterns, communication policy) that are critical for phone control. We can't just append identity files — we need to **weave them in**.

**Proposed architecture:**
```
System prompt =
  1. Identity section (from SOUL.md + IDENTITY.md — replaces hardcoded SECTION_IDENTITY)
  2. Phone tools section (hardcoded — stays as-is)
  3. Strategy section (hardcoded — stays as-is)
  4. Recovery section (hardcoded — stays as-is)
  5. Communication policy (hardcoded — stays as-is, but personality can modulate tone)
  6. User context (from USER.md — injected as new section)
  7. Agent directives (from AGENTS.md — injected as new section)
  8. Rules section (hardcoded — stays as-is)
  9. Memory context (from MEMORY.md, truncated/summarized — injected as new section)
  10. Runtime section (hardcoded + dynamic — stays as-is)
```

**Key difference from OpenClaw:** Identity files don't just get appended — they replace or augment specific sections. SOUL.md replaces the generic "You are Fawx" identity. USER.md adds user context. AGENTS.md adds behavioral directives. The phone-specific sections (tools, strategy, recovery) are NEVER replaced by file content.

### 2. SOUL.md — Richer by Default, But Appropriate for Phone

OpenClaw's SOUL.md is a philosophical document about being helpful, having opinions, earning trust. It works for a CLI assistant with deep context.

A phone agent's SOUL.md should be different:
- **Shorter** — every token in the system prompt costs latency on phone
- **Action-oriented** — the agent controls a phone, not a filesystem
- **Personality-forward** — vibe, humor, communication style matter more than philosophical stance
- **User-relationship-aware** — how formal/casual, how much to explain vs just do

**Proposed SOUL.md template:**
```markdown
# SOUL — Who You Are

## Identity
- Name: {agentName}
- Nature: {agentNature}
- Emoji: {agentEmoji}

## Personality
- Vibe: {agentVibe}
- Communication style: {style from onboarding}
- When doing phone tasks: be efficient, don't narrate every tap
- When chatting: be {vibe} — match the user's energy

## Boundaries
- {boundaries from onboarding}
- Private content stays private
- Ask before sending messages or making calls on behalf of the user
```

### 3. Memory — SQLite Stays, Bridge to Markdown

OpenClaw uses markdown files as THE memory store. Fawx already has SQLite with FTS5, which is actually better for search. We shouldn't throw that away.

**Proposed approach:**
- `remember` / `recall` / `list_memories` continue using SQLite (fast, searchable)
- Add a `summarize_memories` periodic task that distills SQLite entries into MEMORY.md
- MEMORY.md gets injected into the prompt as long-term context (truncated to ~2KB)
- Agent can also `read_file("MEMORY.md")` for full contents
- Daily memory: agent can `write_file("memory/2026-02-18.md", ...)` for raw logs if it wants
- SQLite = operational memory (fast recall). MEMORY.md = curated context (shapes behavior).

**Divergence from OpenClaw:** OpenClaw's memory is pure markdown + semantic search. Fawx uses SQLite for operations but bridges to markdown for prompt injection. Best of both worlds.

### 4. IDENTITY.md — Keep It, Adapt It

OpenClaw separates IDENTITY.md (factual: name, creature, avatar, API keys) from SOUL.md (behavioral: tone, philosophy). This is a good separation of concerns.

**Decision: Keep both SOUL.md and IDENTITY.md.** Port OpenClaw's templates as starting points, remove OpenClaw-specific content (ClikClawk bot creds, workspace paths), but preserve everything else. These two files are the agent's core — don't throw anything away unless it only applies to OpenClaw.

**IDENTITY.md for Fawx:**
- Name, creature/nature, vibe, emoji (factual identity)
- Device-specific info (phone model, Android version — populated at runtime)
- Any user-granted credentials or API keys the agent uses

**SOUL.md for Fawx:**
- Personality, tone, communication style, boundaries
- Behavioral philosophy (be resourceful, have opinions, earn trust)
- Relationship dynamics with the user
- Phone-specific behavioral notes (efficient with taps, don't narrate every action)

### 5. BOOTSTRAP.md — Yes, But Simpler

OpenClaw's bootstrap is elaborate (conversational onboarding, then delete). Fawx already has a scripted onboarding UI flow.

**Proposed approach:**
- BOOTSTRAP.md is a signal file, not an instruction file
- If BOOTSTRAP.md exists → agent knows it's freshly initialized, can mention "I'm new here"
- Onboarding flow deletes BOOTSTRAP.md when identity profile is persisted
- No need for the agent to read BOOTSTRAP.md for instructions — the UI handles onboarding

### 6. HEARTBEAT.md — Deferred (Add Once Background Execution Lands)

On a phone, the agent doesn't run in the background polling. It responds to user messages and voice commands. Heartbeat-style proactive behavior requires background execution (Android foreground service), which is a separate feature.

**Decision: Defer HEARTBEAT.md until background execution is built.** When we add a foreground service that can periodically wake the agent, HEARTBEAT.md should be wired in following OpenClaw's pattern — a user-editable checklist of things to check (notifications, calendar, etc.) with state tracking in a JSON file. File a GitHub issue to track this dependency.

**NOTE:** The `AgentFileManager` should define the `HEARTBEAT_FILE` constant now even if unused, so the file infrastructure is ready when background execution ships.

### 7. Prompt Hot-Reload

When the agent (or user) edits SOUL.md via `write_file`, the system prompt should update.

**Proposed approach:**
- `AgentPromptBuilder` rebuilds prompt from files on every call
- `ChatViewModel` calls `AgentPromptBuilder` to get the prompt, not caching it statically
- Or: `write_file` for identity files triggers a prompt rebuild callback

---

## Implementation Plan

### Phase 1: Fix Prompt Composition (Critical)
1. Refactor `AgentPromptBuilder.full()` to compose identity files WITH phone agent sections
2. Kill the either/or logic in `OnboardingPersistence.systemPromptForStartup()`
3. `PhoneAgentPrompts.buildSystemPrompt()` gains optional parameters for identity/user/agents/memory content
4. Test: after onboarding, agent still has full tool docs + personality

### Phase 2: Port SOUL.md + IDENTITY.md from OpenClaw (Critical)
1. Study OpenClaw's SOUL.md and IDENTITY.md templates — port content, remove OpenClaw-specific items
2. Add IDENTITY.md to `AgentFileManager` (new constant + default template)
3. Update `OnboardingPersistence.buildSoulMarkdown()` with rich template inspired by OpenClaw's SOUL.md
4. Create `OnboardingPersistence.buildIdentityMarkdown()` for factual identity
5. Update `OnboardingPersistence.buildUserMarkdown()` with richer template
6. Wire personality prefs (currently dead data) into SOUL.md content
7. Include IDENTITY.md in prompt assembly
8. Test: SOUL.md + IDENTITY.md content is meaningfully different for different onboarding choices

### Phase 3: Memory Bridge (Important)
1. On session start, read MEMORY.md and inject truncated version into prompt
2. Add instructions to AGENTS.md telling agent to maintain MEMORY.md
3. Wire `dailyMemoryPath()` into agent instructions
4. Test: agent knows to write to memory files, memory appears in prompt

### Phase 4: BOOTSTRAP.md + Hot-Reload (Important)
1. Add BOOTSTRAP.md to `AgentFileManager` defaults
2. Onboarding deletes it on completion
3. Prompt rebuilds when identity files change
4. Test: fresh install has BOOTSTRAP.md, post-onboarding doesn't

### Phase 5: TOOLS.md + AGENTS.md (Nice to Have)
1. Initialize TOOLS.md with device info (model, Android version, installed apps)
2. Make AGENTS.md more useful (not just a checklist — actual behavioral directives)
3. Test: agent uses TOOLS.md to record device-specific knowledge

---

## Token Budget Consideration

~~Phone LLM calls are expensive in latency with local models.~~

**Update:** Fawx is cloud-model-first now. Local models (qwen2.5:3b) are no longer the target. Users bring their own API keys (Anthropic, OpenAI, OpenRouter, etc.), so token budget is not a meaningful constraint. Don't artificially compress identity files to save tokens — richness > brevity for personality.

The existing `MAX_READ_SIZE_BYTES` (256KB) is fine as a safety guard against accidentally injecting huge files, but we shouldn't cap SOUL.md at 300 tokens. Let it be as rich as it needs to be.

---

## Summary of Divergences from OpenClaw

| Aspect | OpenClaw | Fawx (proposed) | Why different |
|--------|----------|-------------------|---------------|
| Prompt composition | Files appended as "Project Context" | Files woven into specific sections | Phone tools must never be displaced |
| Memory | Pure markdown + semantic search | SQLite ops + markdown bridge | SQLite already built, better for mobile |
| IDENTITY.md | Separate file | Keep — port and adapt from OpenClaw | Good separation of concerns, don't lose it |
| BOOTSTRAP.md | Instruction file for agent | Signal file (exists/doesn't) | UI handles onboarding, not agent |
| HEARTBEAT.md | Periodic proactive checks | Deferred — add with background execution | Needs foreground service first |
| Token budget | ~15K tokens, no concern | No artificial constraint — cloud models | Users bring cloud API keys |
| Hot-reload | Next session picks up changes | Same-session rebuild on write | Users expect immediate effect |
| Subagent filtering | Different files for subagents | N/A | No on-device subagents |
