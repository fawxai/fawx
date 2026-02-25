# Citros Onboarding Chat — Design Brief

> Onboarding step after plan selection and before API key setup.
> Feed this to Claude Cowork to generate interactive Jetpack Compose mockups that match the existing Citros visual language.

## Context

This screen is a conversational bootstrap. Instead of another form, the user and Citros establish identity, tone, and boundaries through a short real chat.

It should feel alive and personal, but still deterministic enough to extract structured onboarding data and continue the flow safely.

## Flow Context

```
Welcome → Flavor → Personality → Paywall → **Onboarding Chat (this screen)** → API Key Setup → Permissions → Ready
```

If this step is feature-flagged off, flow proceeds from Paywall directly to API Key Setup with no broken state.

## Canonical Interaction Decisions (must follow)

- This is an onboarding step with visible progress, Back support, and a clear Skip path.
- The conversation must feel natural, but required identity fields still need to be captured before completion.
- Assistant should keep responses short (2-3 sentences), conversational, and non-corporate.
- Completion requires both a hidden completion token and successful structured extraction from transcript.
- Extraction results are shown as editable chips before final confirmation; user always has correction control.
- Skip must never trap the user: it applies safe defaults, captures user name, and continues to API key setup.
- All onboarding chat state persists locally so app restarts can resume without losing context.

## Experience Goals

- Establish emotional trust quickly without feeling scripted.
- Collect practical personalization data needed for post-onboarding prompts.
- Keep the flow fast: most users should finish in about 6-10 exchanges.
- Make completion obvious and reassuring ("Looks good" confirmation moment).

## Screen Layout

### 1. Header
- Title: `Getting to know each other`
- Compact progress indicator (step count or segmented bar)
- `Back` (left) and `Skip` (right) affordances always visible
- Subtle flavor-tinted gradient background (same design language as main chat)

### 2. Conversation Area
- Reuse main chat bubble styles and spacing patterns
- Assistant opens with a short first message after a brief typing delay
- Typing/streaming behavior should match main chat polish (no abrupt message pops)
- Preserve transcript continuity if user leaves and returns

### 3. Identity Summary Chips
- Appears once data starts solidifying
- Example chips:
  - `🏷 Name: Zest`
  - `🎭 Vibe: Chill`
  - `🧑 You: Joe`
- Each chip is tappable to revise inline (bottom sheet or inline editor)
- Unresolved fields should show as `Pending` instead of blank

### 4. Composer
- Same input component family as main chat
- Placeholder: `Type a message...`
- Send button uses active flavor accent
- Optional suggestion chips can appear when user stalls

### 5. Completion Panel
- When minimum required fields are captured, show a clear CTA:
  - `Looks good - continue`
- Secondary action:
  - `Edit details`
- Completion panel should not appear before extraction confidence is sufficient

## Behavior

| Action | Result |
|--------|--------|
| User sends message | Append user bubble, show assistant typing, call onboarding-chat model with transcript context. |
| Assistant message arrives | Render message, parse hidden completion token if present, never show token text to user. |
| Assistant sends `[ONBOARDING_COMPLETE]` | Strip token, run structured extraction pass on transcript, surface editable summary chips. |
| User taps summary chip | Open targeted edit UI, update extracted profile locally, keep transcript intact. |
| User taps `Looks good - continue` | Persist profile, mark onboarding chat complete, navigate to API key setup. |
| User taps `Skip` | Apply defaults + capture minimal user name, mark as skipped, navigate to API key setup. |
| App restarts mid-chat | Reload transcript + partial extraction state and resume from same step. |

## Conversation Prompt Package

### System Prompt (chat model call)

```text
You are a newly activated AI assistant inside the user's phone. This is your first conversation with them.

Goals for this conversation:
1) Establish your identity (name, nature, vibe, emoji)
2) Learn the user's preferred name/address
3) Learn interaction preferences and boundaries
4) Understand what they want help with

Style rules:
- Be conversational, warm, and concise (2-3 sentences).
- Avoid rigid questionnaires, numbered interrogations, and corporate filler.
- Ask one focused question at a time.
- Offer suggestions when the user seems unsure.
- React naturally to user choices.
- Do not mention internal tokens, extraction, or app mechanics.

Completion rules:
- When the basics are clearly covered (usually 6-10 exchanges), send a warm closing message.
- Append [ONBOARDING_COMPLETE] at the very end of that final assistant message.
- Never expose or explain the token.
```

### Structured Extraction Prompt (second pass)

```text
Extract onboarding identity/profile fields from the transcript.

Return strict JSON:
{
  "agent_name": "string|null",
  "agent_nature": "string|null",
  "agent_vibe": "string|null",
  "agent_emoji": "string|null",
  "user_name": "string|null",
  "user_address": "string|null",
  "relationship_style": "string|null",
  "boundaries": "string|null",
  "user_context": "string|null",
  "missing_fields": ["..."],
  "confidence": 0.0
}

Guidelines:
- Use null for unknown values.
- Keep values short and user-meaningful.
- Do not invent details absent from transcript.
- Confidence should reflect extraction reliability for this profile as a whole.
```

## Data to Extract

| Field | Required | Example | Persisted As |
|------|----------|---------|-------------|
| AI name | Yes | "Zest" | `agent_name` |
| AI creature/nature | Yes | "citrus spirit" | `agent_nature` |
| AI vibe | Yes | "chill but sharp" | `agent_vibe` |
| AI emoji | Yes | "🍋" | `agent_emoji` |
| User name | Yes | "Joe" | `user_name` |
| User address preference | Yes | "Joe" | `user_address` |
| Relationship style | Yes | "casual, no corporate speak" | `relationship_style` |
| Boundaries | Yes | "ask before sending or deleting" | `boundaries` |
| User context | Optional | "startup founder, night owl" | `user_context` |

Minimum completion gate: all required fields are non-null and extraction confidence is above threshold.

## Skip Behavior

If user taps `Skip`:
- Persist defaults:
  - `agent_name = "Citros"`
  - `agent_emoji = "🍊"`
  - `agent_nature = "citrus guide"`
  - `agent_vibe = "friendly and helpful"`
- Capture user name with a single field (if not already known)
- Mark chat onboarding as skipped and continue to API key setup immediately

## Persistence (Mock)

Store in onboarding preferences (`citros_onboarding`) for now:

```kotlin
"onboarding_chat_seen" -> true
"onboarding_chat_complete" -> true | false
"onboarding_chat_skipped" -> true | false
"onboarding_chat_transcript" -> "[{role,content,timestamp}, ...]" // JSON string

"agent_name" -> "Zest"
"agent_nature" -> "citrus spirit"
"agent_vibe" -> "chill but sharp"
"agent_emoji" -> "🍋"
"user_name" -> "Joe"
"user_address" -> "Joe"
"relationship_style" -> "casual, no corporate speak"
"boundaries" -> "ask before send/delete/purchase"
"user_context" -> "startup founder, night owl"
```

No backend dependency required for MVP mock flow.

## UI State Contract (Mock)

```kotlin
enum class ExtractionStatus { IDLE, RUNNING, READY, ERROR }

data class IdentitySummary(
    val agentName: String?,
    val agentNature: String?,
    val agentVibe: String?,
    val agentEmoji: String?,
    val userName: String?,
    val userAddress: String?,
    val relationshipStyle: String?,
    val boundaries: String?,
    val userContext: String?,
    val missingFields: List<String>,
    val confidence: Float
)

data class OnboardingChatUiState(
    val messages: List<Message>,
    val isAssistantTyping: Boolean,
    val extractionStatus: ExtractionStatus,
    val summary: IdentitySummary?,
    val isCompletionReady: Boolean,
    val isSkipConfirmOpen: Boolean,
    val hasReducedMotion: Boolean
)
```

## Accessibility & UX Guardrails

- WCAG AA contrast: 4.5:1 normal text, 3:1 large text/icons.
- Do not rely on color alone for chip/status meaning; use labels/icons.
- Minimum touch target: 48dp for chips, skip/back links, send button, and completion CTA.
- Support dynamic type without clipping bubbles, chips, or header controls.
- Respect reduced-motion preference (disable non-essential typing/delay/pulse effects).
- Ensure screen reader announces new assistant messages and chip updates clearly.

## Content Safety Guardrails

- No manipulation, guilt, or dependency language.
- No romantic/sexual framing.
- No pretending legal/medical authority.
- If user gives unsafe boundary instructions, steer to safer defaults and keep onboarding brief.

## Acceptance Checklist

- [ ] Screen placement and progress indicators match onboarding flow with Back + Skip available
- [ ] Conversation feels natural while still collecting all required identity fields
- [ ] Hidden completion token is stripped from visible UI and never shown to user
- [ ] Structured extraction runs on completion and produces editable summary chips
- [ ] Required fields gate completion; user cannot continue with missing required identity data unless they skip
- [ ] Skip applies documented defaults, captures user name, and proceeds without dead ends
- [ ] Transcript and partial extraction state survive process death/app restart
- [ ] Touch targets, contrast, dynamic type, and reduced-motion behaviors meet guardrails
- [ ] Mock aligns visually with existing Citros chat + flavor system (not a separate design language)

## Related Docs

- [Onboarding Spec](onboarding-spec.md) — overall onboarding architecture
- [Paywall Design Brief](ui/paywall-design-brief.md) — prior step and tier persistence
- [MVP UI Design Brief](ui/mvp-ui-design-brief.md) — global chat/settings/interaction system
- [Wallet UI Design Brief](ui/wallet-ui-design-brief.md) — API key flow immediately after this screen
