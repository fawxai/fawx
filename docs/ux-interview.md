# Fawx UX Interview — In Progress

*Started 2026-02-08. Joe is defining the UX vision through structured Q&A.*

---

## Context

The spec treats UI as infrastructure — "overlay bubble," "quick settings tile," "smithay compositor" — but never asks the fundamental question: **what does it feel like to use this thing?**

For a product whose entire thesis is reinventing how humans interact with phones, the interaction design needs its own specs:
- `horizon1-UX-spec.md` — Android PoC experience design
- `horizon2-UX-spec.md` — FawxOS experience design

These will feed into UI mocks for marketing and actual implementation.

---

## Topic 1: Fawx's Personality ✅ ANSWERED

**Q1-Q3: Voice, vibe, verbosity**
> **Joe's decision:** These are NOT pre-set. They are decided through an **interview during the user onboarding flow**. The user shapes Fawx's personality in their first interaction — building a relationship before they've even used the product.

**Q4: Visual identity**
> **Joe's decision:** Fawx's icon is based on a **PLACEHOLDER_SUPERFAWX** image (reference: `docs/assets/PLACEHOLDER_SUPERFAWX-reference.jpg`). During onboarding, the user selects from a set of colors, and their choice determines the color tint of the PLACEHOLDER_SUPERFAWX icon. This makes Fawx visually personal from the start.

### Design implications:
- Onboarding flow must include personality interview (voice character, verbosity level, vibe)
- Onboarding must include color picker that tints the PLACEHOLDER_SUPERFAWX icon
- All UI elements (bubble, overlay, confirmation dialogs) inherit the user's chosen color
- The PLACEHOLDER_SUPERFAWX icon is the universal symbol for Fawx across all states

---

## Topic 2: The Floating Bubble (Horizon 1) ✅ ANSWERED

### Q1: Bubble states

- **Idle** — Small orb, pitch black, slight shine (obsidian-like). Out of the way, not distracting. Haptic feedback on tap.
- **Listening** — Black orb transforms into small glowing star, shifts into user's chosen colorway. Text interface bubble appears on screen for typing.
- **Thinking** — Swirling, spinning. Inner glow pulsating.
- **Acting** — Context-dependent visualization (see Q3). Haptic feedback on a rhythm to indicate Fawx is actively working.
- **Waiting for confirmation** — Slow brightness oscillation (high/low), spending more time at peak and trough, with higher/lower magnitude brightness. Haptic tap every 2 seconds.
- **Error** — Shaking in all directions (not spinning). Red. Tap to see error text. If voice mode on, tapping plays answer aloud.

### Q2: Bubble size and position

- Small and out of the way when idle
- User can move orb anywhere on screen, persists
- Wake by **tap-hold** (default) or **3D press** (supported devices)
- Dismiss by **tap-hold-drag to corner/margin**
- Simple tap = haptic acknowledgment only, does NOT wake

### Q3: When Fawx acts on your phone

Three modes, context-dependent. Fawx auto-selects defaults, user can change in onboarding and settings:

1. **Narrated overlay** — opt-in, off by default. Real-time text describing actions.
2. **PiP preview** — for background tasks or API calls. Small window showing plan summary.
3. **Highlighted touches** — for on-screen app interactions. Fawx hovers over tap targets, visual ripples at touch points, haptic feedback at each "tap" or as letters/words are entered.

In all cases, user can opt in or out of specific settings.

### Q4: Follow-up answers

- **Text interface bubble** appears at same location as idle orb but larger
- **Interruption** — touch and hold anywhere on screen for 2 seconds → Fawx stops
- Tap-hold is primary wake gesture; 3D press is bonus on supported devices

### Design output
→ Full spec: `docs/ui/ui-spec-floating-bubble.md`

---

## Topic 3: Confirmation UX (Trust Model) ✅ ANSWERED

### Permission Tiers
Four tiers, selected during mandatory onboarding, editable in settings:
1. **Full control** — Fawx asks before everything
2. **External + destructive** — asks before sending messages/emails/tweets, deleting (recommended default)
3. **Financial + destructive only** — asks before purchases, finances, file deletion
4. **Autonomous** — Fawx never asks (expert mode)

Default tier decided during onboarding.

### Confirmation UI
- Color coded by risk: **green** (low), **yellow** (medium), **red** (high)
- **Swipe slider at bottom of screen** to confirm (intentional gesture)
- Both one-by-one AND approve-all for batch actions

### Timeout
- 60 seconds → Fawx goes idle
- Pending action saved in prompt window with "Do you still want me to..."
- Idle orb shows faint colored ring indicating pending action

### Design output
→ Full spec: `docs/ui/ui-spec-floating-bubble.md` §4

---

## Topic 4: Sound Design ✅ ANSWERED

### Core principle
Sound follows a **personality-dependent cascade** based on onboarding:

| Priority | Condition | Output |
|----------|-----------|--------|
| 1 | Verbose + volume on | AI-generated speech |
| 2 | Moderate + volume on | Ambient tone/sound |
| 3 | Minimal OR volume off | Haptic only |

Screen-off bumps up one tier (moderate → speaks).

### Key decisions
- Spoken phrases are **AI-generated in real-time** (not pre-recorded)
- Personality and context determine what Fawx says
- Same cascade applies to all sound events (wake, complete, error, confirm)
- Specific tones/sounds to be decided later

### Design output
→ Full spec: `docs/ui/ui-spec-floating-bubble.md` §5

---

## Topic 5: Text & Visual Communication ✅ ANSWERED

- **Text appearance:** Streams in (typing/ChatGPT effect)
- **Typography:** Clean and minimal
- **Overlay background:** Translucent bubble (frosted glass)
- **Theme:** Follows system theme (dark/light)

### Design output
→ Full spec: `docs/ui/ui-spec-floating-bubble.md` §6

---

## Topic 6: Horizon 2 — FawxOS Modes — DEFERRED

*Joe wants to think more before committing. "I'm tempted to keep it
the same for horizon 2 as we have it in 1, but it should have a
different vibe. I'm not sure I'm ready to make these choices yet."*

### Questions for when ready:
- **Ambient mode**: What does the screen show when you're not actively using it?
- **Active mode**: What triggers the transition from ambient?
- **Immersive mode**: Full agent interaction — what's the layout?
- **Review mode**: Looking at what Fawx did while you were away
- Transitions between modes — gestures, animations
- The "awareness surface" concept

---

## Topic 7: Marketing Mocks — NOT YET ASKED

### Questions to ask:
- Which states/moments are most compelling for marketing visuals?
- Do we need a "hero shot" of the bubble on a real phone screen?
- Video demo: real device footage or animated mockup?
- What's the one screenshot that sells Fawx?
