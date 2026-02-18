# System Prompt Lifecycle

This document explains when system prompts are loaded, rebuilt, and which variant is used.

## Trigger Points

### 1. App Startup

When `ChatActivity` launches, the system prompt is resolved via `OnboardingPersistence.systemPromptForStartup()`:

- If `SOUL.md` and `USER.md` both exist and are non-empty → `AgentPromptBuilder.full()` assembles all sections
- Otherwise → falls back to the built-in `PhoneAgentPrompts.SYSTEM_PROMPT`

### 2. Post-Onboarding

After the user completes onboarding:

1. `OnboardingPersistence.persistIdentityProfile()` writes `SOUL.md` and `USER.md`
2. `ChatViewModel.setSystemPrompt()` is called with the new prompt
3. The provider client is rebuilt with the updated system prompt

### 3. Wallet / Model Changes

When the user switches API keys or models via `ChatViewModel.updateModelsFromWallet()`:

- API backends are rebuilt with the new key/model
- The **system prompt stays the same** — it's not regenerated on wallet changes

### 4. Agentic Action Loop

The phone agent action loop uses `AgentPromptBuilder.trimmed()`, which includes only:

- `SOUL.md` — agent identity
- `SECURITY.md` — safety guardrails

This saves tokens by excluding `USER.md`, `AGENTS.md`, `TOOLS.md`, and `MEMORY.md` from action-loop requests.

## Prompt Variants

| Variant | Sections Included | Used By |
|---------|------------------|---------|
| `full()` | SOUL, USER, AGENTS, SECURITY, TOOLS, MEMORY | Main chat conversation |
| `trimmed()` | SOUL, SECURITY | Agentic action loop |
| `SYSTEM_PROMPT` | Built-in default | Pre-onboarding fallback |

## Observability

When a section is skipped (file missing or blank), `AgentPromptBuilder` logs a debug message:

```
D/AgentPromptBuilder: Skipping section USER.md: not readable (File not found: USER.md)
D/AgentPromptBuilder: Skipping section SOUL.md: blank or whitespace-only
```

Filter with: `adb logcat -s AgentPromptBuilder`
