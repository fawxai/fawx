# Citros Floating Overlay - Design Brief

> Overlay mode for in-progress phone actions so users can monitor, interrupt, and queue follow-up instructions without losing context.

## Context

When Citros is executing phone actions (navigation, taps, typing), the full app should not disappear. Instead, it should transition to a compact floating overlay that keeps users informed and in control while the underlying app remains visible.

This brief is optimized for mock generation and implementation handoff.

## Flow Context

```text
Main Chat -> User sends phone-action request -> Mini-Chat Overlay (default)
         -> User can minimize to Bubble or expand to Full App
         -> Action completes/fails -> Result visible without context loss
```

## Canonical Interaction Decisions (must follow)

- Action execution continues across all three surfaces (Mini-Chat, Bubble, Full App) unless the user explicitly taps `Pause` or `Stop`.
- Default transition after action start is `Full App -> Mini-Chat Overlay` using a spring animation (~280ms).
- Mini-Chat starts bottom-anchored but is draggable within safe screen bounds and snaps to nearest edge on release.
- Mini-Chat drag is initiated from the top bar only; chat scroll/input interactions never trigger drag.
- Bubble supports: tap to expand, drag to reposition, long press (stationary) for quick actions (`Stop`, `Expand`, `Dismiss Overlay`).
- `Stop Action` must be high visibility and immediate. After stop, show an `Undo` affordance for 5 seconds.
- On action completion, do not force auto-expand to full app. Keep current surface and show completion status (inline if Mini-Chat, badge if Bubble).
- Full App expanded state must show a persistent "Action in progress" banner with `Return to Overlay`.
- Overlay must remain usable with keyboard open, orientation changes, and split-screen.

## Experience Goals

- `Always visible state`: users can always tell what Citros is doing.
- `Low obstruction`: overlay should not block core app content more than necessary.
- `Fast interruption`: stopping or pausing an action should be one step.
- `Continuity`: moving between mini, bubble, and full app should preserve chat/action context.
- `Confidence`: risk and progress cues should be readable at a glance.

## Surface Specs

### 1) Floating Mini-Chat (default during execution)

- Container: rounded rectangle, ~36-42% screen height, bottom anchored on entry.
- Visual style: dark frosted surface with citrus accents; subtle active glow while executing.
- Header row:
  - left: Citros avatar
  - center: current step text (`Opening Settings...`, `Scrolling Wi-Fi list...`)
  - right: `Expand` and `Minimize` icon buttons
- Body:
  - compact scrollable chat stream (latest system/user/action lines)
  - inline progress chip (`Step 2 of 5`)
- Composer:
  - single-line "Queue a follow-up..." field
  - send icon button
- Action controls:
  - prominent `Stop Action` button
  - optional secondary `Pause` text action

### 2) Floating Status Bubble (minimized)

- Size: ~56dp circular bubble.
- Content: Citros avatar with activity ring when executing.
- Badge: unread count or dot if new status/messages arrive while minimized.
- Interactions:
  - tap: expand to Mini-Chat
  - drag: reposition bubble
  - long press (stationary): quick actions menu (`Stop`, `Expand`, `Dismiss Overlay`)

### 3) Full App (expanded from overlay)

- Existing full-screen chat UI.
- Persistent in-progress banner pinned near top:
  - `Citros is executing actions`
  - primary action: `Return to Overlay`
  - secondary action: `Stop`
- Action stream and queued messages remain visible and synchronized with overlay state.

## Behavior Matrix

| User Action | Expected Result |
|---|---|
| Send phone-action request in full app | Full app transitions into Mini-Chat overlay; execution starts/continues. |
| Tap `Minimize` in Mini-Chat | Collapse to Bubble at same approximate screen region. |
| Tap Bubble | Expand to Mini-Chat with prior scroll/input state preserved. |
| Long press Bubble | Open quick actions menu; choosing `Stop` ends execution and shows undo. |
| Type queued message in Mini-Chat | Message enters queue and appears in stream as "Queued". |
| Tap `Stop Action` | Execution stops immediately; inline status changes to `Stopped`; show 5s undo. |
| Expand to Full App during execution | Execution continues; banner provides `Return to Overlay`. |
| Action completes in Bubble mode | Bubble shows completion badge; no forced full-screen takeover. |
| Action fails | Error state appears inline (Mini-Chat/Full App) or as bubble badge + tooltip summary. |

## Visual and Motion Direction

- Theme: dark-first, citrus accents (tangerine/amber/lime), no neon glow overload.
- Shape: 24dp radius for Mini-Chat container, pill chips for status/progress.
- Typography: Android-native sans (13-14sp compact body in Mini-Chat, 16sp header/status).
- Motion:
  - transform full -> mini: spring (~280ms)
  - mini <-> bubble: scale + translate spring (~220ms)
  - stop/undo feedback: quick fade-slide (~160ms)
- Active indicator should use both motion and iconography, not color alone.

## Accessibility and UX Guardrails

- Contrast: WCAG AA minimums (4.5:1 text, 3:1 large text/icons).
- Touch targets: all interactive controls >= 48dp.
- Never rely on color alone for state (pair with icon/label text).
- Reduced motion mode:
  - disable pulsing borders/rings
  - replace with static status icon + text
- Dynamic type support: no clipped status text, input, or control labels.
- Screen reader semantics:
  - announce mode changes (`Mini-Chat`, `Bubble`, `Full App`)
  - announce action-state changes (`Executing`, `Paused`, `Stopped`, `Completed`, `Failed`)
  - clearly label destructive action (`Stop Action`)

## Technical Reality Constraints (for realistic mocks)

- Overlay permission: `SYSTEM_ALERT_WINDOW` (`TYPE_APPLICATION_OVERLAY`).
- Rendering path: Compose in overlay service (`ComposeView` + `WindowManager`).
- Must account for:
  - keyboard/IME insets
  - cutouts/safe areas
  - split-screen and orientation changes
- Minimized mode may use Bubble API on Android 11+ where appropriate.

## UI State Contract (Mock)

```kotlin
enum class OverlaySurface { MINI_CHAT, BUBBLE, FULL_APP }
enum class AgentRunState { EXECUTING, PAUSED, STOPPING, COMPLETED, FAILED }

data class FloatingOverlayUiState(
    val surface: OverlaySurface,
    val runState: AgentRunState,
    val currentStepLabel: String,
    val queuedMessageDraft: String,
    val queuedMessageCount: Int,
    val unreadStatusCount: Int,
    val isKeyboardVisible: Boolean,
    val isQuickActionsMenuVisible: Boolean,
    val isUndoStopVisible: Boolean,
    val canReturnToOverlay: Boolean,
    val isReducedMotion: Boolean
)
```

## Required Mock Outputs

Produce all of the following:

1. Mini-Chat running state (default position, active progress).
2. Mini-Chat with keyboard open while user types: `also check bluetooth`.
3. Mini-Chat dragged to a non-bottom location with snap behavior shown.
4. Bubble active state with spinner ring.
5. Bubble long-press quick actions menu open.
6. Full App expanded state with persistent in-progress banner and `Return to Overlay`.
7. Completion state while still in Bubble mode (badge + summary affordance).
8. Failure state (error summary + recovery affordance) without losing queue context.
9. Transition storyboard for:
   - Full App -> Mini-Chat
   - Mini-Chat -> Bubble -> Mini-Chat
   - Stop Action -> Undo feedback

For each screen, provide mobile frame variants at `360x800` and `412x915`.

## Acceptance Checklist

- [ ] Three surfaces are visually cohesive and preserve Citros branding.
- [ ] Core behavior is unambiguous: execution continues unless explicitly paused/stopped.
- [ ] Drag, tap, and long-press gesture precedence is clear and conflict-free.
- [ ] `Stop Action` is obvious, immediate, and has undo protection.
- [ ] Completion/failure behaviors do not force disruptive auto-expansion.
- [ ] Keyboard, orientation, and split-screen constraints are represented in mocks.
- [ ] Accessibility gates are satisfied (contrast, touch targets, non-color cues, reduced motion, screen reader labels).
- [ ] Mock outputs include all required states and transition storyboards.

## Copy/Paste Prompt (for mock generation)

Use this exact prompt with your design/mock tool:

```text
Design a production-ready Android floating overlay system for Citros (AI phone agent) with three synchronized surfaces: Mini-Chat Overlay, Status Bubble, and Full App.

Goal: while Citros executes phone actions, users can monitor progress, queue follow-up instructions, and interrupt instantly without losing context.

Canonical behavior:
- Execution continues across all surfaces unless user explicitly pauses/stops.
- Default transition is Full App -> Mini-Chat when action starts.
- Mini-Chat is bottom-anchored on entry, draggable by top bar only, and snaps to screen edges.
- Bubble interactions: tap expand, drag reposition, long press quick actions (Stop, Expand, Dismiss Overlay).
- Stop Action is immediate and must show a 5-second Undo affordance.
- Completion/failure should not force full-app takeover; preserve current surface with clear status.

Include these required frames:
1) Mini-Chat executing
2) Mini-Chat with keyboard open and queued text "also check bluetooth"
3) Mini-Chat dragged and snapped
4) Bubble active with status ring
5) Bubble quick actions menu
6) Full App with in-progress banner + Return to Overlay
7) Completion while in Bubble mode
8) Failure state with recovery affordance
9) Transition storyboard: full->mini, mini<->bubble, stop->undo

Visual direction:
- Dark frosted overlay, citrus accent palette, 24dp rounded corners, compact readable typography.
- Motion should be springy but restrained.
- Must look Android-native and implementation-feasible.

Accessibility constraints:
- 4.5:1 text contrast, 48dp minimum touch targets, non-color status cues, reduced-motion variant, and explicit screen reader labels for mode and run-state changes.

Output both 360x800 and 412x915 frame variants.
```

## Related Docs

- [MVP UI Design Brief](mvp-ui-design-brief.md)
- [Onboarding Chat Design Brief](onboarding-chat-design-brief.md)
- [Wallet UI Design Brief](wallet-ui-design-brief.md)
