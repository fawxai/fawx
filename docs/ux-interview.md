# Nova UX Interview — In Progress

*Started 2026-02-08. Joe is defining the UX vision through structured Q&A.*

---

## Context

The spec treats UI as infrastructure — "overlay bubble," "quick settings tile," "smithay compositor" — but never asks the fundamental question: **what does it feel like to use this thing?**

For a product whose entire thesis is reinventing how humans interact with phones, the interaction design needs its own specs:
- `horizon1-UX-spec.md` — Android PoC experience design
- `horizon2-UX-spec.md` — NovaOS experience design

These will feed into UI mocks for marketing and actual implementation.

---

## Topic 1: Nova's Personality ✅ ANSWERED

**Q1-Q3: Voice, vibe, verbosity**
> **Joe's decision:** These are NOT pre-set. They are decided through an **interview during the user onboarding flow**. The user shapes Nova's personality in their first interaction — building a relationship before they've even used the product.

**Q4: Visual identity**
> **Joe's decision:** Nova's icon is based on a **supernova** image (reference: `docs/assets/supernova-reference.jpg`). During onboarding, the user selects from a set of colors, and their choice determines the color tint of the supernova icon. This makes Nova visually personal from the start.

### Design implications:
- Onboarding flow must include personality interview (voice character, verbosity level, vibe)
- Onboarding must include color picker that tints the supernova icon
- All UI elements (bubble, overlay, confirmation dialogs) inherit the user's chosen color
- The supernova icon is the universal symbol for Nova across all states

---

## Topic 2: The Floating Bubble (Horizon 1) ✅ ANSWERED

### Q1: Bubble states

- **Idle** — Small orb, pitch black, slight shine (obsidian-like). Out of the way, not distracting. Haptic feedback on tap.
- **Listening** — Black orb transforms into small glowing star, shifts into user's chosen colorway. Text interface bubble appears on screen for typing.
- **Thinking** — Swirling, spinning. Inner glow pulsating.
- **Acting** — Context-dependent visualization (see Q3). Haptic feedback on a rhythm to indicate Nova is actively working.
- **Waiting for confirmation** — Slow brightness oscillation (high/low), spending more time at peak and trough, with higher/lower magnitude brightness. Haptic tap every 2 seconds.
- **Error** — Shaking in all directions (not spinning). Red. Tap to see error text. If voice mode on, tapping plays answer aloud.

### Q2: Bubble size and position

- Small and out of the way when idle
- User can move orb anywhere on screen, persists
- Wake by **tap-hold** (default) or **3D press** (supported devices)
- Dismiss by **tap-hold-drag to corner/margin**
- Simple tap = haptic acknowledgment only, does NOT wake

### Q3: When Nova acts on your phone

Three modes, context-dependent. Nova auto-selects defaults, user can change in onboarding and settings:

1. **Narrated overlay** — opt-in, off by default. Real-time text describing actions.
2. **PiP preview** — for background tasks or API calls. Small window showing plan summary.
3. **Highlighted touches** — for on-screen app interactions. Nova hovers over tap targets, visual ripples at touch points, haptic feedback at each "tap" or as letters/words are entered.

In all cases, user can opt in or out of specific settings.

### Q4: Follow-up answers

- **Text interface bubble** appears at same location as idle orb but larger
- **Interruption** — touch and hold anywhere on screen for 2 seconds → Nova stops
- Tap-hold is primary wake gesture; 3D press is bonus on supported devices

### Design output
→ Full spec: `docs/ui-spec-floating-bubble.md`

---

## Topic 3: Confirmation UX (Trust Model) — NOT YET ASKED

The policy engine is the most important safety feature. The user-facing side — how you approve or reject an action — needs to be a beautifully designed moment.

### Questions to ask:
- What information should a confirmation show? (action description, consequences, undo option?)
- Full-screen takeover or inline in the bubble?
- Swipe to confirm (like Apple Pay) or tap?
- How does Nova communicate risk level? (routine vs high-stakes visual distinction)
- Timeout behavior — if user doesn't respond, does Nova wait forever or cancel?
- Batch confirmations — "I want to do 5 things, approve all?" or one-by-one?

---

## Topic 4: Sound Design — NOT YET ASKED

For a voice-first device, sound is critical.

### Questions to ask:
- Wake word acknowledgment sound (the "I heard you" moment)
- Completion sound (task done)
- Error tone
- Confirmation request sound (attention needed)
- Ambient awareness audio (subtle chime for background task completion?)
- Should sounds have the same personality customization as the voice?

---

## Topic 5: Text & Visual Communication — NOT YET ASKED

### Questions to ask:
- When Nova shows text on the overlay, how does it appear? All at once, streaming/typing, animated?
- Typography personality — clean and minimal? Warm and rounded? Technical and monospaced?
- Does the overlay have a background or is it transparent floating text?
- Dark mode only, light mode only, or follows system?

---

## Topic 6: Horizon 2 — NovaOS Modes — NOT YET ASKED

The spec mentions "ambient/active/immersive/review modes" but never defines them.

### Questions to ask:
- **Ambient mode**: What does the screen show when you're not actively using it? Clock + context? Visualization of agent state? Minimal text?
- **Active mode**: What triggers the transition from ambient? How does it look/feel different?
- **Immersive mode**: Full agent interaction — what's the layout?
- **Review mode**: Looking at what Nova did while you were away — timeline? Cards? Conversation log?
- Transitions between modes — what are the gestures? The animations?
- The "awareness surface" concept — screen as something you glance at rather than interact with. What does that actually look like?

---

## Topic 7: Marketing Mocks — NOT YET ASKED

### Questions to ask:
- Which states/moments are most compelling for marketing visuals?
- Do we need a "hero shot" of the bubble on a real phone screen?
- Video demo: real device footage or animated mockup?
- What's the one screenshot that sells Nova?
