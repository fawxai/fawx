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

## Topic 2: The Floating Bubble (Horizon 1) — PENDING

*Joe is thinking on this. Questions below for when he returns.*

The bubble is the primary UI surface in Horizon 1 — always present on Android, always communicating state.

### Q1: Bubble states
The supernova icon needs to communicate what Nova is doing:
- **Idle** — floating, available. Subtle glow? Dim? Slowly pulsing?
- **Listening** — wake word triggered. Bright flare? Expanding rings? Color shift?
- **Thinking** — processing request. Spinning? Swirling nebula animation? Inner glow?
- **Acting** — executing on the phone. Progress trail? Step-by-step overlay?
- **Waiting for confirmation** — needs approval (the trust moment). Distinct from other states?
- **Error/failed** — something went wrong. Red shift? Shake? Collapse animation?

### Q2: Bubble size and position
- Small and out of the way (like a chat head), or larger and more present?
- Does it resize based on state (expand when active, shrink when idle)?

### Q3: When Nova is acting on your phone
"Watching it tap itself like a ghost is unsettling" — what should the user see instead?
- **Narrated overlay** — Nova describes what it's doing in real-time ("Opening Messages → Finding Sarah → Typing...")
- **Picture-in-picture preview** — small window showing plan summary being executed
- **Highlighted touches** — visual ripples where Nova "taps," making it feel intentional
- **Hands-off mode** — screen goes to a Nova "working" animation and comes back when done
- A combination?

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
