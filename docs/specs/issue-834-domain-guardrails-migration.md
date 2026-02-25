# Issue #834 — Domain Guardrails Migration

## Summary
Issue #834 de-tacticalizes domain-specific web guardrails by defaulting to **generic** recovery behavior and keeping legacy tactical guidance behind an explicit compatibility flag.

## Tactical heuristics inventory (before migration)

| Area | Tactical behavior | Status after migration |
|---|---|---|
| Prompt strategy (`PhoneAgentPrompts.SECTION_STRATEGY`) | Explicit web triage rules (`Search vs Browse vs Chrome`, `Never open Chrome just to search`) | Compatibility-gated (`COMPATIBILITY`) |
| Prompt recovery (`PhoneAgentPrompts.SECTION_RECOVERY`) | Web-specific failure playbooks (`Web search failed`, `Web browse failed`) | Compatibility-gated (`COMPATIBILITY`) |
| Tool error text (`WebSearchClient`) | Anti-browser directives in no-results / provider-failure strings | Compatibility-gated (`COMPATIBILITY`) |

## What changed
- Added `PhoneAgentPrompts.DomainGuardrailMode`:
  - `GENERIC` (default)
  - `COMPATIBILITY`
- Prompt builders now accept `domainGuardrailMode`:
  - `PhoneAgentPrompts.buildSystemPrompt(...)`
  - `PhoneAgentPrompts.buildActionPrompt(...)`
  - `AgentPromptBuilder.full(...)`
  - `AgentPromptBuilder.trimmed(...)`
- Runtime wiring in `PhoneAgentApi`:
  - New constructor flag: `domainGuardrailMode`
  - The same mode is passed to both prompt construction and `WebSearchClient` fallback text.
- `WebSearchClient` now emits generic fallback text by default; tactical anti-browser wording is only emitted in compatibility mode.

## Behavior matrix

| Model tier | GENERIC (default) | COMPATIBILITY |
|---|---|---|
| STANDARD / FLAGSHIP | Generic strategy + generic recovery guidance. No tactical Chrome/web directives. | Restores legacy tactical strategy + recovery guidance for web fallback scenarios. |
| SMALL | Uses `SECTION_STRATEGY_SMALL` + generic recovery guidance. | Same as GENERIC for prompt sections (mode intentionally ignored for SMALL tier to keep behavior deterministic and compact). |

Note: SMALL tier also excludes research tools (`web_search`, `web_fetch`, `web_browse`) via tool gating, so tactical web fallback directives are not applied there.

## Rationale
Generic loop controls (runtime constraints + deterministic fallback/state handling) should own fallback behavior. Prompt- and tool-level domain tactics are now optional compatibility behavior instead of default policy.

## Rollback path
If regressions appear in tactical web scenarios, switch to compatibility mode at agent construction:

```kotlin
PhoneAgentApi(
    chatClient = chatClient,
    actionClient = actionClient,
    domainGuardrailMode = PhoneAgentPrompts.DomainGuardrailMode.COMPATIBILITY
)
```

This restores prior tactical web guardrails without reverting code.

## Regression coverage
- `PhoneAgentPromptsTest`: verifies default generic behavior and compatibility restoration.
- `WebSearchClientTest`: verifies default generic fallback text and compatibility anti-browser text across no-results and provider-failure paths.
