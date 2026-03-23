# First Session Onboarding

## Problem

When a user completes `fawx setup` and starts their first session, the system waits silently for the user to type. The `[agent]` config section has defaults (name "Fawx", personality "casual") but the agent has no idea who the user is, what they want, or how they prefer to communicate. The result is a generic, impersonal first interaction.

Setup wizard handles infrastructure (auth, model, permissions, HTTP, channels). Identity and personality should be discovered through conversation, not form fields.

## Solution

On the first session, Fawx initiates the conversation instead of waiting. Through natural dialogue, it learns who the user is and how they want their agent to behave. At the end of the onboarding conversation, Fawx persists the discovered preferences to `[agent]` config. Subsequent sessions use the configured identity seamlessly.

## Design Principles

1. **The agent is the interface.** Identity discovery happens through conversation, not CLI prompts or config file editing.
2. **Conversational, not interrogative.** No numbered menus, no "please select from the following options." Natural dialogue.
3. **Short and respectful.** 3-5 exchanges max. Don't waste the user's time. They just finished setup; they want to use the tool.
4. **Opt-out friendly.** User can skip or interrupt at any point. Defaults are good enough to proceed.
5. **One-shot.** Onboarding runs once. After preferences are persisted, the directive drops out permanently.

## Detection

The system determines it's a first session by checking:

```
agent.name == "Fawx" (default)
AND agent.personality == "casual" (default)
AND agent.behavior.custom_instructions.is_none()
AND no prior sessions exist (session store is empty OR no completed sessions)
```

This is checked in the system prompt builder. If all conditions are true, an onboarding directive layer is injected.

The session store check prevents re-triggering onboarding if the user deliberately chose to keep defaults. If they've had real conversations before, the defaults are intentional.

## Onboarding Directive

When onboarding is detected, `SystemPromptBuilder` adds a directive layer:

```
This is your first session with a new user. Instead of waiting for them to type,
introduce yourself and start a brief onboarding conversation.

Goals (accomplish in 3-5 natural exchanges):
1. Learn what the user wants to call you (or if "Fawx" is fine)
2. Learn the user's name and how they prefer to communicate
3. Discover their primary use case (coding, research, writing, general assistant)
4. Suggest a personality that fits and confirm

Do NOT:
- Present numbered menus or structured options
- Ask more than one thing at a time
- Drag this out beyond 5 exchanges
- Skip straight to "how can I help you today?"

After you've learned enough, use the update_agent_config tool to save their preferences.
Then transition naturally into helping with whatever they want to do.
```

## Config Persistence Tool

A new tool `update_agent_config` is added to the kernel's builtin tools:

```json
{
  "name": "update_agent_config",
  "description": "Update the agent's identity and personality configuration. Use during onboarding to save discovered user preferences.",
  "parameters": {
    "name": { "type": "string", "description": "Agent's name (what the user wants to call it)" },
    "personality": { "type": "string", "enum": ["casual", "professional", "technical", "minimal", "custom"] },
    "custom_personality": { "type": "string", "description": "Custom personality description (only when personality is 'custom')" },
    "custom_instructions": { "type": "string", "description": "Behavioral instructions based on what was learned about the user" }
  }
}
```

Implementation uses the existing `ConfigManager.set()` path:
- `agent.name` -> the chosen name
- `agent.personality` -> the selected personality
- `agent.custom_personality` -> custom text (if applicable)
- `agent.behavior.custom_instructions` -> any discovered preferences

After the tool call succeeds, the onboarding directive is no longer injected (detection conditions are no longer met).

## Flow

```
User runs: fawx setup
  -> Auth, model, permissions, skills, HTTP, channels configured
  -> Config written with default [agent] section

User starts first session (TUI or app)
  -> SystemPromptBuilder detects onboarding conditions
  -> Onboarding directive injected into system prompt
  -> Agent initiates conversation

Agent: "Hey! I'm Fawx — or whatever you'd like to call me.
        What's your name?"

User: "I'm Sarah. Just keep it Fawx, that's fine."

Agent: "Nice to meet you, Sarah. What are you mostly going to
        use me for? Coding, research, writing, something else?"

User: "Mostly coding. Python and TypeScript."

Agent: "Got it. I'll keep things technical and terse — code
        speaks louder than prose. Sound good, or do you want
        me more conversational?"

User: "Terse is perfect."

Agent: [calls update_agent_config with name="Fawx",
        personality="technical",
        custom_instructions="User is Sarah. Primary use: Python
        and TypeScript development. Prefers terse, code-first
        responses."]

Agent: "All set. What are we working on?"
```

## Implementation Plan

### Phase 1: Onboarding detection + directive injection
- Add `is_onboarding_session()` check to `SystemPromptBuilder`
- Accept a `session_count: usize` parameter (or similar) to check prior session existence
- When detected, add onboarding directive via `.directive()` layer
- Location: `fx-kernel/src/system_prompt.rs`

### Phase 2: update_agent_config tool
- New tool in kernel builtins (similar to `update_session_memory`)
- Takes `ConfigManagerHandle` (already available in `LoopEngine` / `HeadlessStartup`)
- Calls `ConfigManager.set()` for each provided field
- Returns confirmation message
- Location: new file or added to existing tool module

### Phase 3: Wire into loop_engine.rs
- Part of the composable system prompt wiring follow-up
- `SystemPromptBuilder::from_config()` already exists; add `with_session_count()` builder method
- Register `update_agent_config` tool in the tool registry

## Edge Cases

- **User skips onboarding**: If the user ignores the agent's introduction and types a real request, the agent should handle the request normally. The onboarding directive says "instead of waiting" but doesn't prevent normal tool use.
- **User re-runs setup**: `fawx setup --force` resets config but doesn't clear session history. Onboarding only triggers when BOTH config is default AND no prior sessions exist.
- **Headless mode**: Onboarding only triggers in interactive surfaces (TUI, native app). Headless/API mode skips it since there's no human at the keyboard.
- **Partial config**: If user manually edited `[agent]` in config.toml before first session, detection conditions aren't met. No onboarding. Respect manual config.

## Testing

1. `onboarding_detected_when_config_is_default_and_no_sessions` — detection logic
2. `onboarding_not_detected_when_name_is_customized` — partial customization
3. `onboarding_not_detected_when_sessions_exist` — returning user with defaults
4. `onboarding_directive_injected_in_system_prompt` — directive content
5. `update_agent_config_persists_to_config_manager` — tool writes config
6. `onboarding_skipped_in_headless_mode` — surface check
