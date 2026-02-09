# Nova UI Spec — The Floating Bubble

*Horizon 1: Android PoC Primary UI Surface*

> **Purpose:** This spec defines the visual design, interaction model,
> and animation behavior for Nova's floating bubble — the always-present
> interface element on the user's Android phone. Hand this to a UI
> designer to produce high-fidelity mockups and animation prototypes.

---

## 1. Design Foundation

### 1.1 Visual Identity

Nova's icon is a **supernova** — a celestial orb that transforms
between states. During onboarding, the user selects a color from a
curated palette. This color becomes their personal Nova colorway,
applied across all UI elements (bubble, overlays, text, confirmation
dialogs).

Reference image: `docs/assets/supernova-reference.jpg`

### 1.2 Personality

Nova's voice, verbosity, and vibe are **not preset**. They are
determined through an interview during the user's first-run onboarding
flow. The user shapes Nova's personality before they've used it —
building a relationship from the first interaction.

**Onboarding is mandatory — it cannot be skipped.**

### 1.3 Design Principles

- **Ambient, not intrusive** — Nova should feel like a presence, not
  a notification
- **State is always visible** — the bubble's appearance tells the user
  what Nova is doing without requiring interaction
- **Personal** — the user's chosen color and personality make Nova
  feel like *theirs*
- **Trustworthy** — confirmation moments are clear, unhurried, and
  never sneaky

---

## 2. Bubble Anatomy

### 2.1 Idle State

The default, resting state. Nova is available but not active.

| Property | Value |
|----------|-------|
| Shape | Small circular orb |
| Color | Pitch black |
| Surface | Slight reflective shine (like obsidian) |
| Animation | None — completely still |
| Size | Small, comparable to an Android chat head (~48-56dp) |
| Position | User-defined — draggable to any screen edge/area, persists across sessions |
| Presence | Out of the way, not distracting |

**Variant — Pending Action:**
When Nova timed out waiting for confirmation (see §2.5) or has an
undelivered notification, the idle orb displays a **faint colored
ring** in the user's colorway. This signals "I have something for
you" without being intrusive.

**Interaction:**
- **Tap-hold** (default) or **3D press** (on supported devices) to
  wake Nova → transitions to Listening state
- Simple tap triggers haptic feedback (acknowledgment) but does NOT
  wake Nova — prevents accidental activation
- 3D press is a bonus on supported hardware; tap-hold is the
  universal fallback

### 2.2 Listening State

Nova has been woken and is ready to receive input.

| Property | Value |
|----------|-------|
| Shape | Small star (transforms from orb) |
| Color | Black → user's chosen colorway (animated transition) |
| Animation | Gentle glow/shimmer in user's color. Star-like radiance. |
| Size | Same position as idle orb, slightly larger to indicate active state |

**Visual changes:**
1. Black orb morphs into a small, glowing star shape
2. Color shifts from pitch black to the user's onboarding colorway
3. A **text interface bubble** appears on screen at the same location
   as the idle orb but larger — provides a text input field for users
   who prefer typing over speaking
4. Text bubble inherits the user's colorway for accent elements

**Audio feedback (personality-dependent cascade):**
- **Screen off:** Nova speaks an acknowledgment phrase, AI-generated
  based on personality (e.g., "Listening", "What's up?", "Hit me",
  "How can I help?")
- **Screen on, verbose setting:** Nova speaks
- **Screen on, moderate setting:** Ambient vibrating flux sound
- **Screen on, minimal setting / volume off:** Haptic feedback only

**Interaction:**
- User speaks (voice input) or types in the text bubble
- Submitting input transitions to Thinking state

### 2.3 Thinking State

Nova is processing the user's request.

| Property | Value |
|----------|-------|
| Shape | Orb/star (retains activated form) |
| Color | User's colorway |
| Animation | **Swirling + spinning** rotation. Inner glow **pulsating** — rhythmic intensity changes. |
| Size | Same as listening state |

**Animation detail:**
- Outer surface swirls like a nebula — particles or light trails
  rotating around the center
- Inner core pulsates with light, breathing in and out
- Conveys active computation without implying a specific duration
- Should feel energetic but controlled — not frantic

### 2.4 Acting State

Nova is executing actions on the phone.

| Property | Value |
|----------|-------|
| Shape | Orb/star |
| Color | User's colorway |
| Animation | Context-dependent (see §3 Action Visualization) |
| Haptics | Rhythmic haptic feedback pattern — indicates Nova is actively working |

**Haptic pattern:**
- Subtle, rhythmic pulses while Nova works
- Not continuous vibration — a beat that says "I'm doing things"
- Cadence should feel purposeful and steady

### 2.5 Waiting for Confirmation State

Nova needs the user's approval before proceeding. This is the
**trust moment** — the most important state to get right.

| Property | Value |
|----------|-------|
| Shape | Orb/star |
| Color | Risk-level dependent (see §4 Permission System) |
| Animation | **Slow brightness oscillation** — high-to-low-to-high, spending more time at peak and trough brightness than in transition. Higher peak and lower trough than other animated states. |
| Haptics | Single tap every **2 seconds** — persistent, attention-seeking |

**Animation detail:**
- Brightness cycle is deliberately slow and dramatic — designed to
  catch the user's peripheral vision
- Peak brightness is notably brighter than thinking state
- Trough is notably dimmer — the contrast range is wide
- Lingers at peak and trough (ease-in-out with long holds at extremes)
- The overall effect: a slow, deep "breathing" that says
  "I need you"

**Confirmation UI:**
- A **swipe slider at the bottom of the screen** to confirm
- Swipe gesture is intentional — harder to accidentally approve
  than a tap
- Slider color matches risk level (green/yellow/red)

**Timeout behavior:**
- After **60 seconds** with no response, Nova returns to idle
- The pending action is saved in the text prompt window with a note:
  *"Do you still want me to..."*
- Idle orb shows **faint colored ring** indicating pending action
  (see §2.1 Variant)

**Batch confirmations:**
- Both **one-by-one** and **approve all** options available
- "Approve all" shown when Nova has multiple related actions queued

### 2.6 Error State

Something went wrong.

| Property | Value |
|----------|-------|
| Shape | Orb |
| Color | **Red** (overrides user colorway) |
| Animation | **Shaking in all directions** — erratic, not spinning. Conveys distress/failure. |
| Size | Same as active states |

**Interaction:**
- Tap to see error message in text overlay
- If user has voice/talk mode enabled, tapping plays the error
  explanation aloud
- After viewing/hearing the error, bubble returns to idle state

**Animation detail:**
- Random directional micro-movements (not a clean oscillation)
- Feels like something is wrong — distinct from all other states
- Red color is unmistakable even in peripheral vision

---

## 3. Action Visualization

When Nova executes actions on the phone, the visual feedback depends
on context. Nova **auto-selects the appropriate mode** based on action
type, with the user able to override defaults in onboarding and
settings.

### 3.1 Highlighted Touches (Default for On-Screen Interactions)

Used when Nova interacts with visible app UI elements (tapping buttons,
typing, scrolling).

| Element | Behavior |
|---------|----------|
| Nova orb | Hovers over / near the tap target |
| Tap indicator | Visual ripple at each touch point |
| Haptics | Subtle tap at each "press" by Nova |
| Text input | Light tapping sensation as characters are entered |

**Design goals:**
- Feel intentional, not ghostly
- The user can see WHERE Nova is interacting
- Haptic feedback makes each action tangible
- Should feel like watching a skilled hand work, not a haunted phone

### 3.2 Picture-in-Picture Preview (Default for Background/API Tasks)

Used when the action can happen via API calls or in the background
(sending a message via API, checking weather, querying data).

| Element | Behavior |
|---------|----------|
| Display | Small PiP window showing summary of the plan being executed |
| Content | Step list or progress indicator |
| Position | Near the bubble, non-obstructive |

**Design goals:**
- User sees what's happening without losing their current screen
- Compact and dismissible
- Shows plan steps completing in sequence

### 3.3 Narrated Overlay (Opt-In, Off by Default)

Real-time text narration of Nova's actions.

| Element | Behavior |
|---------|----------|
| Display | Text overlay describing each step |
| Content | "Opening Messages → Finding Sarah → Typing..." |
| Default | **OFF** — user must opt in via settings |

**Design goals:**
- Maximum transparency for users who want it
- Useful for learning what Nova can do
- Should not obscure the actions themselves

### 3.4 Mode Selection Logic

```
Action type              → Default mode
─────────────────────────────────────────
On-screen app interaction → Highlighted Touches
Background / API task     → PiP Preview
User opted into narration → Narrated Overlay (additive)
```

Users can change defaults in:
- Onboarding flow (initial preference)
- Settings menu (anytime)

---

## 4. Permission System

### 4.1 Permission Tiers

Four tiers, selected by the user during mandatory onboarding.
Editable anytime in settings.

| Tier | Name | Nova asks before... |
|------|------|---------------------|
| 1 | **Full control** | Everything — every action requires approval |
| 2 | **External + destructive** (recommended default) | Sending messages/emails/tweets, deleting files/data |
| 3 | **Financial + destructive only** | Purchases, accessing finances, deleting files |
| 4 | **Autonomous** | Nothing — Nova acts freely (expert mode) |

The default tier is determined during onboarding based on the user's
comfort level.

### 4.2 Risk-Level Color Coding

Confirmation UI color reflects the stakes:

| Risk Level | Color | Examples |
|------------|-------|----------|
| Low | **Green** | Open an app, search something, read info |
| Medium | **Yellow** | Send a message, post on social media, edit a file |
| High | **Red** | Delete files, make a purchase, access financial accounts |

The confirmation slider, bubble glow, and any overlay elements all
reflect the risk color during the confirmation state.

### 4.3 Confirmation Interaction

1. Nova enters Confirmation state (slow brightness oscillation +
   haptic taps)
2. Confirmation card appears showing:
   - What Nova wants to do (action description)
   - Risk level indicator (color coded)
3. User **swipes the slider at the bottom of the screen** to approve
4. Or taps "Cancel" / waits 60 seconds for auto-dismiss

---

## 5. Sound & Audio Design

### 5.1 Personality-Dependent Cascade

All audio follows a three-tier cascade based on user settings
from onboarding:

| Priority | Condition | Output |
|----------|-----------|--------|
| 1 | Verbose personality + volume on | **AI-generated speech** (contextual, personality-matched) |
| 2 | Moderate personality + volume on | **Ambient tone/sound** |
| 3 | Minimal personality OR volume off | **Haptic feedback only** |

Screen-off interactions bump up one tier (e.g., moderate → speaks)
to compensate for no visual feedback.

### 5.2 Sound Events

| Event | Verbose | Moderate | Minimal |
|-------|---------|----------|---------|
| Wake acknowledgment | AI-generated phrase ("What's up?", "Listening", etc.) | Vibrating ambient flux sound | Haptic only |
| Task complete | AI-generated ("Done!", "All set", etc.) | Subtle chime | Haptic only |
| Error | AI-generated ("That didn't work", etc.) | Alert tone | Haptic only |
| Confirmation needed | AI-generated ("Need your OK", etc.) | Attention chime | Haptic only |
| Background task done | AI-generated notification | Ambient chime | Haptic only |

### 5.3 AI-Generated Speech

Spoken responses are **generated in real-time by Nova's LLM**, not
pre-recorded or from a fixed set. This means:
- Phrases match the user's chosen personality and verbosity
- Responses are contextually appropriate (not the same thing every time)
- Voice character (warm, professional, playful, etc.) is set during
  onboarding

---

## 6. Text & Visual Communication

### 6.1 Text Rendering

| Property | Value |
|----------|-------|
| Text appearance | **Streams in** (typing/ChatGPT effect) |
| Typography | **Clean and minimal** |
| Overlay background | **Translucent bubble** behind text |
| Theme | **Follows system theme** (dark/light) |

### 6.2 Text Bubble Design

- Translucent background with slight blur (frosted glass effect)
- User's colorway as accent (borders, highlights, cursor)
- Clean sans-serif typeface
- Text streams in character-by-character or word-by-word for
  conversational feel
- Static text (labels, buttons) appears immediately

---

## 7. Interaction Model

### 7.1 Waking Nova

| Gesture | Behavior |
|---------|----------|
| Tap-hold on idle orb | Wake → Listening state (default on all devices) |
| 3D press on idle orb | Wake → Listening state (supported devices only) |
| Simple tap on idle orb | Haptic acknowledgment only — does NOT wake (prevents accidental activation) |
| Wake word (voice) | Wake → Listening state (no touch required) |

### 7.2 Dismissing / Minimizing Nova

| Gesture | Behavior |
|---------|----------|
| Tap-hold-drag to corner/margin | Dismiss → returns to Idle state |

The dismiss gesture mirrors Android's chat head pattern — familiar
to users.

### 7.3 Interrupting Nova Mid-Action

| Gesture | Behavior |
|---------|----------|
| Touch and hold **anywhere on screen** for **2 seconds** | Nova immediately pauses/stops current action |

**Design considerations:**
- 2-second hold prevents accidental interrupts from normal phone use
- Nova should acknowledge the interrupt (visual + haptic feedback)
- After interrupt, Nova enters a "paused" state where user can
  choose to resume, cancel, or give new instructions

### 7.4 Moving the Bubble

| Gesture | Behavior |
|---------|----------|
| Drag (while idle) | Reposition orb anywhere on screen |
| Release | Orb stays at new position, persists across app switches and sessions |

---

## 8. State Transition Map

```
                    tap-hold / 3D press / wake word
            ┌──────────────────────────────────────┐
            │                                      ▼
         ┌──────┐                           ┌───────────┐
         │ IDLE │                           │ LISTENING  │
         └──────┘                           └───────────┘
            ▲                                      │
            │ dismiss / error viewed         user submits input
            │ / 60s timeout                        │
            │                                      ▼
         ┌──────┐     needs approval      ┌───────────┐
         │ERROR │◄────────────────────────│ THINKING   │
         └──────┘     (on failure)        └───────────┘
            ▲                              │         │
            │                    action    │         │ needs
            │                    ready     │         │ approval
            │                              ▼         ▼
            │                        ┌─────────┐ ┌───────────┐
            └────────────────────────│ ACTING  │ │ CONFIRMING│
                   (on failure)      └─────────┘ └───────────┘
                                          │           │
                                          │  approved  │
                                          │◄───────────┘
                                          │
                                     task complete
                                          │
                                          ▼
                                       ┌──────┐
                                       │ IDLE │
                                       └──────┘

Note: CONFIRMING → IDLE after 60s timeout (saves pending action)
      IDLE with faint ring = has pending action or notification
```

---

## 9. Color & Theming

| Element | Color Source |
|---------|-------------|
| Idle orb | Pitch black with subtle shine |
| Idle orb (pending) | Pitch black + faint colored ring (user colorway) |
| Active states (listening, thinking, acting) | User's chosen colorway |
| Confirming state | Risk-level color (green/yellow/red) |
| Error state | Red (overrides colorway) |
| Text interface bubble | Translucent + colorway accents, follows system theme |
| Highlighted touch ripples | Colorway with transparency |
| PiP preview | Translucent + colorway accents, follows system theme |
| Confirmation slider | Risk-level color |

---

## 10. Haptic Language

| Event | Haptic Pattern |
|-------|---------------|
| Tap idle orb (no wake) | Single short tap — acknowledgment |
| Wake (tap-hold complete) | Distinct "click" — state changed |
| Each Nova tap during action | Subtle tap — mirroring the action |
| Text input by Nova | Light rapid tapping — typing sensation |
| Working rhythm (acting state) | Steady rhythmic pulse |
| Confirmation needed | Single tap every 2 seconds |
| Error | Sharp double-tap |
| Interrupt acknowledged | Strong single tap — "I stopped" |

---

## 11. Responsive Sizing

| State | Approximate Size |
|-------|-----------------|
| Idle | ~48-56dp (Android chat head scale) |
| Listening | ~64-72dp (slightly larger, plus text bubble) |
| Thinking | ~64-72dp |
| Acting | ~64-72dp (plus action visualization overlay) |
| Confirming | ~64-72dp (plus confirmation slider at bottom) |
| Error | ~64-72dp |

The orb itself doesn't dramatically resize — state is communicated
through color, animation, and supplementary UI elements (text bubble,
PiP, overlays, confirmation slider).

---

## 12. Onboarding Flow (UI-Relevant Decisions)

The following are decided during the mandatory onboarding interview
and affect all UI behavior:

| Decision | Affects |
|----------|---------|
| **Color selection** | All colorway elements across every state |
| **Personality interview** | Voice character, spoken phrase style |
| **Verbosity level** | Sound cascade tier (verbose/moderate/minimal) |
| **Permission tier** | Which actions require confirmation |
| **Action visualization preference** | Override defaults for action modes |

---

## 13. Accessibility Considerations

- All states must be distinguishable without color alone (animation
  patterns differ per state)
- Haptic patterns should be configurable (intensity, on/off)
- Text interface always available (not voice-only)
- Error messages available as text regardless of voice mode setting
- Screen reader compatibility for all interactive elements
- Confirmation state must be perceivable through multiple channels
  (visual + haptic + optional sound)
- System theme support ensures readability in both light and dark modes

---

## 14. Open Questions

### Topic 6: Horizon 2 — NovaOS Modes
*Deferred — Joe wants to think more before committing to full-screen
modes (ambient/active/immersive/review). Will revisit when design
direction crystallizes.*

### Topic 7: Marketing Mocks
*Not yet discussed — hero shots, video demo style, device branding.*

---

*Spec version: 2.0 — 2026-02-09*
*Based on UX interview with Joe (Topics 1-5, Topic 6 deferred)*
