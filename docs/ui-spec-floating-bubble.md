# Citros UI Spec — The Floating Fruit

*Horizon 1: Android PoC Primary UI Surface*

> **Purpose:** This spec defines the visual design, interaction model,
> animation behavior, and 3D rendering approach for Citros's floating
> bubble — a **3D citrus fruit** that serves as the always-present
> interface element on the user's Android phone. Hand this to a UI
> designer or 3D artist to produce high-fidelity mockups, shader
> prototypes, and animation rigs.

---

## 1. Design Foundation

### 1.1 Visual Identity

Citros's icon is a **3D citrus fruit** — a whole, spherical piece of
fruit rendered with realistic peel texture (dimpled pores, subtle
surface irregularities) and an inner glow effect that makes it look
lit from within. Think of holding a real lime up to a bright light —
you can see the luminance bleeding through the skin. That's the vibe.

During onboarding, the user selects a **flavor** from a curated set
of citrus fruits. This choice sets their color, peel texture, and
personality vibe across all UI elements (bubble, overlays, text,
confirmation dialogs).

The fruit is NOT:
- A flat circle with a gradient
- A cross-section / slice graphic
- A cartoon or emoji fruit
- A smooth, textureless sphere

It IS:
- A whole 3D fruit with visible peel pores and dimples
- Realistic enough to read as "citrus" at a glance
- Stylized enough to feel like a premium UI element, not a photo
- Lit from within — alive, glowing, slightly magical

### 1.2 Personality

Citros's voice, verbosity, and vibe are **not preset**. They are
determined through an interview during the user's first-run onboarding
flow. The user shapes Citros's personality before they've used it —
building a relationship from the first interaction.

The chosen **flavor** subtly inflects personality defaults (a Blood
Orange might default slightly more intense; a Lime might default more
chill) — but the user always has the final say.

**Onboarding is mandatory — it cannot be skipped.**

### 1.3 Design Principles

- **Alive, not intrusive** — Citros should feel like a living thing
  resting on your screen, not a notification badge. The slow idle spin
  and soft glow give it organic presence.
- **State is always visible** — the fruit's glow intensity, spin speed,
  and color shifts tell the user what Citros is doing without requiring
  interaction.
- **Personal** — the user's chosen flavor (fruit type + color + texture)
  makes Citros feel like *theirs*. No two setups feel the same.
- **Trustworthy** — confirmation moments are clear, unhurried, and
  never sneaky.
- **Tactile** — everything should feel like it has weight, texture,
  and physics. The fruit wobbles when you set it down, shakes when
  something's wrong, spins when it's thinking.

---

## 2. Bubble Anatomy

### 2.1 The 3D Fruit Surface

Every state of the bubble shares these foundational properties:

| Property | Value |
|----------|-------|
| **Geometry** | Sphere (slightly oblate for some flavors — see §9 Flavors) |
| **Surface** | Realistic citrus peel: dimpled pores, micro-bumps, subtle specular highlights |
| **Lighting** | Dual-source: (1) ambient environmental light for surface realism, (2) inner volumetric glow that bleeds through the peel |
| **Size** | ~48–56dp at idle (Android chat head scale) |
| **Position** | User-defined — draggable to any screen location, persists across sessions |

The peel texture is **critically important** — it's what makes
rotation visible (a smooth sphere's spin is invisible), gives 3D depth
at small sizes, and differentiates flavors from each other.

### 2.2 Idle State

The default, resting state. Citros is available but not active.

| Property | Value |
|----------|-------|
| **Inner glow** | Soft, barely luminous — like a nightlight behind frosted glass. ~15% intensity. |
| **Rotation** | Very slow spin (~1 revolution per 30 seconds). Barely perceptible — but your eye catches the dimples moving. Axis: slight tilt off vertical for organic feel. |
| **Surface** | Full peel detail visible. Subtle specular highlight from ambient light. |
| **Color** | Flavor's natural hue, slightly desaturated. The glow adds warmth from within. |
| **Size** | ~48–56dp |
| **Presence** | Calm, alive, breathing. Like a fruit sitting on your desk that happens to glow faintly. |

**Variant — Pending Action:**
When Citros timed out waiting for confirmation (see §2.5) or has an
undelivered notification, the fruit's inner glow pulses with a slow
heartbeat rhythm (~1 pulse per 3 seconds). The glow color shifts
slightly warmer (toward amber) to signal "I have something for you."

**Interaction:**
- **Tap-hold** (default) or **3D press** (on supported devices) to
  wake Citros → transitions to Listening state
- Simple tap → haptic feedback (acknowledgment) + brief glow flash,
  but does NOT wake Citros — prevents accidental activation
- 3D press is a bonus on supported hardware; tap-hold is the
  universal fallback

### 2.3 Listening State

Citros has been woken and is ready to receive input.

| Property | Value |
|----------|-------|
| **Inner glow** | Brightens from core outward, ~60% intensity. Light bleeds through peel pores like backlit skin. |
| **Scale** | Fruit subtly swells ~10% over 300ms (ease-out). Like it just took a breath. |
| **Rotation** | Pauses or slows to near-stop — Citros is paying attention, holding still. |
| **Color** | Flavor's full saturated hue. Vivid. Juicy. |
| **Surface** | Peel pores become more visible as light pushes through them from inside — each dimple becomes a tiny point of light. |

**Visual transition:**
1. Inner glow ramps from 15% → 60% over 400ms
2. Fruit swells slightly (scale 1.0 → 1.1)
3. A **text interface bubble** appears adjacent — provides a text
   input field for users who prefer typing over speaking
4. Text bubble inherits flavor colorway for accent elements

**Audio feedback (personality-dependent cascade):**
- **Screen off:** Citros speaks an acknowledgment phrase, AI-generated
  based on personality (e.g., "Listening", "What's up?", "Hit me",
  "How can I help?")
- **Screen on, verbose setting:** Citros speaks
- **Screen on, moderate setting:** Fresh, bright tone — a crisp
  citrusy "pop" sound (see §5 Sound Design)
- **Screen on, minimal setting / volume off:** Haptic feedback only

**Interaction:**
- User speaks (voice input) or types in the text bubble
- Submitting input transitions to Thinking state

### 2.4 Thinking State

Citros is processing the user's request.

| Property | Value |
|----------|-------|
| **Inner glow** | Pulsing — rhythmic intensity oscillation between 40% and 80%. Like a heartbeat of light. |
| **Rotation** | Faster spin (~1 revolution per 3 seconds). The peel dimples make this clearly visible — you can SEE it thinking. |
| **Color** | Flavor's full hue with pulsing luminance |
| **Surface** | Peel texture catches and releases light as it spins — each dimple winks as it rotates through the light. |
| **Scale** | Returns to base size (swell from Listening settles back) |

**Animation detail:**
- Glow pulse: sinusoidal, ~1.5 second cycle (inhale 750ms, exhale 750ms)
- Spin acceleration: ease-in from listening's near-stop to full thinking speed over 500ms
- The combination of pulsing glow + visible spin = clearly "something is happening inside"
- Should feel energetic but controlled — not frantic. A fruit rolling purposefully.

### 2.5 Speaking / Active State

Citros is responding verbally or executing actions on the phone.

| Property | Value |
|----------|-------|
| **Inner glow** | Full radiance — 100% intensity. The whole fruit glows bright. Juicy. |
| **Rotation** | Moderate steady spin. Confident, not frantic. |
| **Color** | Peak saturation of flavor hue. This is the fruit at its most vivid. |
| **Surface** | Fully illuminated — every pore visible, specular highlights dance across the peel. |
| **Haptics** | Rhythmic haptic feedback pattern — indicates Citros is actively working |

**Speaking sub-state:**
When Citros is talking, the glow intensity subtly modulates with the
audio waveform — brighter on emphasized syllables, dimmer on pauses.
The fruit appears to pulse with its own voice.

**Acting sub-state (executing phone actions):**
- Haptic: subtle rhythmic pulses while Citros works
- Visual: context-dependent (see §3 Action Visualization)

### 2.6 Waiting for Confirmation State

Citros needs the user's approval before proceeding. This is the
**trust moment** — the most important state to get right.

| Property | Value |
|----------|-------|
| **Inner glow** | Slow, deep brightness oscillation — 20% ↔ 90%. Lingers at peak and trough. |
| **Rotation** | Stops. The fruit is still, facing you. Waiting. |
| **Color** | Glow shifts to risk-level color (green/amber/red) bleeding through the natural peel color. The fruit's own hue mixes with the risk indicator. |
| **Haptics** | Single tap every **2 seconds** — persistent, attention-seeking |

**Animation detail:**
- Brightness cycle: ~4 seconds total. Ease-in-out with long holds
  at extremes (1s rise, 1s hold bright, 1s fall, 1s hold dim)
- The slow deep breathing is designed to catch peripheral vision
- Risk color bleeds through peel — green glow through orange peel
  looks warm and safe; red glow through orange peel looks urgent and
  hot. Each flavor × risk-color combination should be tested for
  readability.

**Confirmation UI:**
- A **swipe slider at the bottom of the screen** to confirm
- Swipe gesture is intentional — harder to accidentally approve
  than a tap
- Slider color matches risk level (green/amber/red)

**Timeout behavior:**
- After **60 seconds** with no response, Citros returns to idle
- The pending action is saved in the text prompt window with a note:
  *"Do you still want me to..."*
- Idle fruit shows pending-action heartbeat glow (see §2.2 Variant)

**Batch confirmations:**
- Both **one-by-one** and **approve all** options available
- "Approve all" shown when Citros has multiple related actions queued

### 2.7 Needs Attention State

Citros has a notification, completed background task, or information
the user should see — but it's not an error or confirmation request.

| Property | Value |
|----------|-------|
| **Inner glow** | Warm amber/golden glow replaces normal color — like the fruit is ripening. ~50% intensity, gentle pulse. |
| **Rotation** | Gentle wobble — a slight oscillating tilt, like a fruit nudging you. |
| **Color** | Warm amber/gold bleeding through natural peel color |

### 2.8 Error State

Something went wrong.

| Property | Value |
|----------|-------|
| **Inner glow** | Dims to ~10%, desaturated. The fruit looks dull, almost bruised. |
| **Rotation** | **Shake** — quick, erratic micro-movements in random directions. Like a fruit rattling on a table. |
| **Color** | Desaturated flavor hue shifting toward grey-brown. Not red — that's for risk levels. Errors look *unwell*, not alarming. |
| **Size** | Same as active states |

**Interaction:**
- Tap to see error message in text overlay
- If user has voice/talk mode enabled, tapping plays the error
  explanation aloud
- After viewing/hearing the error, bubble returns to idle state

**Animation detail:**
- Shake: random directional micro-movements (±3dp) at ~30Hz for 1.5s,
  then settles with a wobble (see §2.9 Motion Library)
- Desaturation transition: 500ms ease-out
- The fruit looks sick, not angry. Something went wrong inside it.

---

## 2.9 Motion Library

Three core motion primitives that leverage the peel texture for
maximum expressiveness:

### Spin
- **Purpose:** Conveys activity level. A smooth sphere's spin is
  invisible; citrus peel dimples make rotation **unmistakably clear**.
- **Idle:** ~1 revolution / 30s (barely perceptible — your eye catches
  the dimples drifting). Axis tilted ~15° off vertical.
- **Thinking:** ~1 revolution / 3s. Clearly spinning. Dimples stream
  past like stars.
- **Active:** ~1 revolution / 5s. Steady, confident.
- **Easing:** All spin changes use ease-in-out over 500ms. Never
  abrupt starts or stops.

### Shake
- **Purpose:** Alerts, errors, interruptions. Physical and immediate.
- **Implementation:** Random-direction micro-translations (±3dp) at
  ~30Hz. NOT a clean sine wave — deliberately chaotic.
- **Feel:** Like a fruit rolling/rattling on a table. Playful for
  minor alerts, distressed for errors.
- **Duration:** 0.5s (alert) to 1.5s (error).

### Wobble
- **Purpose:** Settling after interaction. Physical satisfaction.
- **Implementation:** Damped rotational oscillation — tilts ±8° with
  decreasing amplitude over 600ms. Like you just set a fruit down and
  it rocks to a stop.
- **Triggers:** After any state transition back to idle. After drag
  repositioning. After shake completes.
- **Physics:** Spring-damper with ~0.6 damping ratio. Overshoots once,
  settles quickly. Satisfying.

---

## 3. Action Visualization

When Citros executes actions on the phone, the visual feedback depends
on context. Citros **auto-selects the appropriate mode** based on action
type, with the user able to override defaults in onboarding and
settings.

### 3.1 Highlighted Touches (Default for On-Screen Interactions)

Used when Citros interacts with visible app UI elements (tapping buttons,
typing, scrolling).

| Element | Behavior |
|---------|----------|
| Citros fruit | Hovers over / near the tap target — still glowing, still textured |
| Tap indicator | Ripple at each touch point, colored in user's flavor |
| Haptics | Subtle tap at each "press" by Citros |
| Text input | Light tapping sensation as characters are entered |

**Design goals:**
- Feel intentional, not ghostly
- The user can see WHERE Citros is interacting
- Haptic feedback makes each action tangible
- Should feel like watching a skilled hand work, not a haunted phone

### 3.2 Picture-in-Picture Preview (Default for Background/API Tasks)

Used when the action can happen via API calls or in the background
(sending a message via API, checking weather, querying data).

| Element | Behavior |
|---------|----------|
| Display | Small PiP window showing summary of the plan being executed |
| Content | Step list or progress indicator |
| Position | Near the fruit, non-obstructive |
| Style | Frosted glass card with flavor-colored accents |

**Design goals:**
- User sees what's happening without losing their current screen
- Compact and dismissible
- Shows plan steps completing in sequence

### 3.3 Narrated Overlay (Opt-In, Off by Default)

Real-time text narration of Citros's actions.

| Element | Behavior |
|---------|----------|
| Display | Text overlay describing each step |
| Content | "Opening Messages → Finding Sarah → Typing..." |
| Default | **OFF** — user must opt in via settings |

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

| Tier | Name | Citros asks before... |
|------|------|---------------------|
| 1 | **Full control** | Everything — every action requires approval |
| 2 | **External + destructive** (recommended default) | Sending messages/emails/tweets, deleting files/data |
| 3 | **Financial + destructive only** | Purchases, accessing finances, deleting files |
| 4 | **Autonomous** | Nothing — Citros acts freely (expert mode) |

The default tier is determined during onboarding based on the user's
comfort level.

### 4.2 Risk-Level Color Coding

Confirmation UI color reflects the stakes. These risk colors **glow
through the fruit's natural peel** — they don't replace it.

| Risk Level | Glow Color | Examples |
|------------|------------|----------|
| Low | **Green** | Open an app, search something, read info |
| Medium | **Amber** | Send a message, post on social media, edit a file |
| High | **Red** | Delete files, make a purchase, access financial accounts |

The confirmation slider, inner glow, and any overlay elements all
reflect the risk color during the confirmation state.

### 4.3 Confirmation Interaction

1. Citros enters Confirmation state (slow brightness oscillation +
   haptic taps, fruit stops spinning)
2. Confirmation card appears showing:
   - What Citros wants to do (action description)
   - Risk level indicator (color coded)
3. User **swipes the slider at the bottom of the screen** to approve
4. Or taps "Cancel" / waits 60 seconds for auto-dismiss

---

## 5. Sound & Audio Design

### 5.1 Sound Character

Citros sounds are **fresh, bright, and crisp** — inspired by the
sensory experience of citrus. Think of the *pop* of puncturing an
orange peel, the *fizz* of sparkling citrus water, the *snap* of
breaking a lemon off a branch.

**NOT:** cosmic hums, deep bass drones, dark ambient, sci-fi beeps.

**YES:** zesty pops, bright crystalline tones, effervescent fizz,
woody snaps, juicy percussive sounds.

### 5.2 Personality-Dependent Cascade

All audio follows a three-tier cascade based on user settings
from onboarding:

| Priority | Condition | Output |
|----------|-----------|--------|
| 1 | Verbose personality + volume on | **AI-generated speech** (contextual, personality-matched) |
| 2 | Moderate personality + volume on | **Citrus tone** (see §5.3) |
| 3 | Minimal personality OR volume off | **Haptic feedback only** |

Screen-off interactions bump up one tier (e.g., moderate → speaks)
to compensate for no visual feedback.

### 5.3 Sound Events

| Event | Verbose | Moderate | Minimal |
|-------|---------|----------|---------|
| Wake acknowledgment | AI-generated phrase ("What's up?", "Listening", etc.) | Bright citrus pop — like puncturing peel | Haptic only |
| Task complete | AI-generated ("Done!", "All set", etc.) | Crisp, satisfying snap — like a clean peel | Haptic only |
| Error | AI-generated ("That didn't work", etc.) | Dull thud — a fruit dropped on the table | Haptic only |
| Confirmation needed | AI-generated ("Need your OK", etc.) | Two-tone ascending chime — bright, expectant | Haptic only |
| Background task done | AI-generated notification | Soft effervescent fizz | Haptic only |
| Flavor selection (onboarding) | Unique pop per flavor | Unique pop per flavor | Haptic only |

### 5.4 AI-Generated Speech

Spoken responses are **generated in real-time by Citros's LLM**, not
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
| Typography | **Clean and minimal** — rounded sans-serif to match the organic fruit aesthetic |
| Overlay background | **Frosted glass bubble** behind text |
| Theme | **Follows system theme** (dark/light) |

### 6.2 Text Bubble Design

- Frosted glass background with blur (translucent, organic feel)
- User's flavor color as accent (borders, highlights, cursor)
- Rounded sans-serif typeface (e.g., Inter, SF Pro Rounded, or similar)
- Text streams in word-by-word for conversational feel
- Static text (labels, buttons) appears immediately
- Subtle flavor-colored glow behind the text bubble to visually tie
  it to the fruit

---

## 7. Interaction Model

### 7.1 Waking Citros

| Gesture | Behavior |
|---------|----------|
| Tap-hold on fruit | Wake → Listening state (default on all devices) |
| 3D press on fruit | Wake → Listening state (supported devices only) |
| Simple tap on fruit | Haptic acknowledgment + brief glow flash — does NOT wake (prevents accidental activation) |
| Wake word ("Hey Citros") | Wake → Listening state (no touch required) |

### 7.2 Dismissing / Minimizing Citros

| Gesture | Behavior |
|---------|----------|
| Tap-hold-drag to corner/margin | Dismiss → fruit wobbles to idle position and settles (wobble animation) |

The dismiss gesture mirrors Android's chat head pattern — familiar
to users.

### 7.3 Interrupting Citros Mid-Action

| Gesture | Behavior |
|---------|----------|
| Touch and hold **anywhere on screen** for **2 seconds** | Citros immediately pauses/stops current action |

**Design considerations:**
- 2-second hold prevents accidental interrupts from normal phone use
- Citros acknowledges: fruit shakes briefly, glow dims, haptic feedback
- After interrupt, Citros enters a "paused" state where user can
  choose to resume, cancel, or give new instructions

### 7.4 Moving the Fruit

| Gesture | Behavior |
|---------|----------|
| Drag (while idle) | Reposition fruit anywhere on screen — fruit tilts slightly in drag direction (physics!) |
| Release | Fruit wobbles to rest at new position (wobble animation), persists across app switches and sessions |

---

## 8. State Transition Map

```
                    tap-hold / 3D press / "Hey Citros"
            ┌──────────────────────────────────────┐
            │                                      ▼
         ┌──────┐                           ┌───────────┐
         │ IDLE │                           │ LISTENING  │
         │ 🍊   │                           │ 🍊💡      │
         └──────┘                           └───────────┘
            ▲                                      │
            │ dismiss / error viewed         user submits input
            │ / 60s timeout                        │
            │                                      ▼
         ┌──────┐     on failure          ┌───────────┐
         │ERROR │◄────────────────────────│ THINKING   │
         │ 🍊💀 │                         │ 🍊🔄      │
         └──────┘                         └───────────┘
            ▲                              │         │
            │                    action    │         │ needs
            │                    ready     │         │ approval
            │                              ▼         ▼
            │                        ┌─────────┐ ┌───────────┐
            └────────────────────────│ ACTIVE  │ │ CONFIRMING│
                   (on failure)      │ 🍊✨    │ │ 🍊⏳      │
                                     └─────────┘ └───────────┘
                                          │           │
                                          │  approved  │
                                          │◄───────────┘
                                          │
                                     task complete
                                          │
                                          ▼
                                       ┌──────┐
                                       │ IDLE │
                                       │ 🍊   │
                                       └──────┘

Note: CONFIRMING → IDLE after 60s timeout (saves pending action)
      IDLE with heartbeat glow = has pending action or notification
```

---

## 9. Flavors — The Personalization System

Flavors are Citros's personalization mechanic. Choosing a flavor sets
your **color**, **peel texture**, **shape**, and **personality vibe**.
This isn't a color picker — it's choosing your fruit.

### 9.1 Flavor Catalog

| Flavor | Emoji | Color | Peel Texture | Shape | Vibe |
|--------|-------|-------|-------------|-------|------|
| **Lime** | 💚 | Bright green | Dense, small bumps — slightly rough | Perfectly spherical | Fresh, sharp, no-nonsense |
| **Tangerine** | 🧡 | Classic orange | Textbook citrus peel — medium dimples, satisfying regularity | Round, slightly flattened at poles | Warm, friendly, reliable |
| **Lemon** | 💛 | Vibrant yellow | Smoother peel with fine, subtle pores | Slightly elongated (ovoid) | Bright, energetic, zingy |
| **Blood Orange** | ❤️ | Deep red-orange | Medium peel with occasional dark speckles | Round, dense-looking | Bold, dramatic, intense |
| **Grapefruit** | 💗 | Pink-coral | Larger, more visible pores — coarser texture | Slightly larger sphere | Warm, generous, easygoing |

### 9.2 Texture Differentiation

At 48–56dp, subtle texture differences must still read clearly:

- **Lime:** Highest-frequency bumps. Surface looks slightly rough/matte
  even at small scale. Catches light in many small specular points.
- **Tangerine:** Medium-frequency dimples. The "default citrus" feel.
  Balanced matte/specular. Most familiar, most readable.
- **Lemon:** Lowest-frequency texture — almost smooth with subtle
  undulation. More specular/glossy than others. Slight elongation
  breaks the sphere.
- **Blood Orange:** Similar to tangerine but with micro-variation in
  color across the surface — darker patches visible even at small
  scale. Moody.
- **Grapefruit:** Largest pore size. At small scale reads as a
  rougher, more organic surface. Warmer specular highlights.

### 9.3 Flavor Selection UX (Onboarding)

1. Five fruits presented in a horizontal carousel — each rendered in
   3D, slowly spinning so the user can see the peel texture
2. Tap a fruit to select — it rolls forward, swells slightly, and
   glows from within (preview of the Listening state)
3. Each fruit plays its unique sound on selection (a signature "pop")
4. Selected fruit does a satisfying wobble-settle
5. "This is your Citros" confirmation — the fruit is now yours

---

## 10. Color & Theming

| Element | Color Source |
|---------|-------------|
| Idle fruit | Flavor's natural hue, slightly desaturated, soft inner glow |
| Idle fruit (pending) | Flavor hue + warm amber heartbeat glow |
| Active states (listening, thinking, active) | Flavor's full saturated hue, bright inner glow |
| Confirming state | Risk-level glow (green/amber/red) bleeding through natural peel |
| Needs attention | Warm amber/gold glow through natural peel |
| Error state | Desaturated flavor hue, dim glow, grey-brown shift |
| Text interface bubble | Frosted glass + flavor-colored accents, follows system theme |
| Highlighted touch ripples | Flavor color with transparency |
| PiP preview | Frosted glass + flavor-colored accents, follows system theme |
| Confirmation slider | Risk-level color (green/amber/red) |

---

## 11. Haptic Language

| Event | Haptic Pattern |
|-------|---------------|
| Tap idle fruit (no wake) | Single short tap — acknowledgment |
| Wake (tap-hold complete) | Distinct "click" — state changed |
| Each Citros tap during action | Subtle tap — mirroring the action |
| Text input by Citros | Light rapid tapping — typing sensation |
| Working rhythm (active state) | Steady rhythmic pulse |
| Confirmation needed | Single tap every 2 seconds |
| Error | Sharp double-tap |
| Interrupt acknowledged | Strong single tap — "I stopped" |
| Drag release (reposition) | Soft thud — "I landed" |

---

## 12. Responsive Sizing

| State | Approximate Size |
|-------|-----------------|
| Idle | ~48–56dp (Android chat head scale) |
| Listening | ~58–66dp (subtle swell, plus text bubble) |
| Thinking | ~52–60dp (slightly larger than idle, returned from swell) |
| Active | ~52–60dp (plus action visualization overlay) |
| Confirming | ~52–60dp (plus confirmation slider at bottom) |
| Error | ~48–56dp (same as idle — diminished, not expanded) |

The fruit itself doesn't dramatically resize — state is communicated
through glow intensity, spin speed, color shifts, and supplementary
UI elements (text bubble, PiP, overlays, confirmation slider).

---

## 13. 3D Rendering Approach

### 13.1 The Challenge

Rendering a convincing 3D textured sphere at 48–56dp on Android with
smooth real-time animation (spin, glow modulation, scale) while
maintaining battery efficiency and low memory footprint.

### 13.2 Recommended Approach: Pre-rendered Sprite Sheets + Shader Overlay

A hybrid approach that balances visual quality with runtime performance:

**Layer 1 — Pre-rendered Fruit Base (sprite sheet):**
- Render each flavor's fruit in a 3D tool (Blender / Cinema 4D) at
  high quality: realistic peel, subsurface scattering for the
  translucent-peel glow effect, physically-based lighting
- Export as a **sprite sheet of rotation frames**: 60 frames covering
  360° of Y-axis rotation (6° per frame). Each frame is 128×128px
  (2x the display size for sharpness on high-DPI screens).
- At runtime, animate by cycling through frames at variable speed:
  idle = 2fps, thinking = 20fps, active = 12fps
- Total memory per flavor: 60 frames × 128×128 × RGBA = ~3.75MB
  (acceptable; can compress to ~1MB with GPU texture compression)

**Layer 2 — Real-time Glow Shader (GPU fragment shader):**
- A simple radial gradient shader applied as a **multiply/screen
  blend** over the sprite. Parameters:
  - `glowIntensity` (0.0–1.0): controlled by state
  - `glowColor` (vec3): flavor color or risk-level color
  - `glowCenter` (vec2): slight offset from center for organic feel
  - `glowPulse` (float): sinusoidal modulator for thinking state
- This gives us smooth, 60fps glow animation without pre-rendering
  every intensity level
- Risk-level color overlay: blend risk color into the glow shader's
  `glowColor` during confirmation state

**Layer 3 — Transform Animations (standard Android):**
- Scale (swell/shrink): `ObjectAnimator` on the `View`'s `scaleX`/`scaleY`
- Position (shake/wobble): `ObjectAnimator` on `translationX`/`translationY`
  with spring interpolator
- Rotation tilt: `ObjectAnimator` on `rotationX`/`rotationY` for
  the wobble settle

### 13.3 Alternative Approaches (Evaluated)

**Full real-time 3D (OpenGL ES / Vulkan):**
- Pros: True 3D, any angle, dynamic lighting, most flexible
- Cons: Battery drain, GPU wake-lock, complex implementation,
  overkill for a 56dp element. Would need a textured sphere mesh +
  PBR shader + subsurface scattering approximation.
- Verdict: **Not recommended for Horizon 1.** Revisit for CitrosOS
  full-screen modes.

**Lottie / Rive animation:**
- Pros: Designer-friendly, easy to iterate, small file size
- Cons: Lottie is 2D — can fake 3D rotation but peel texture won't
  read correctly during spin. Rive has limited 3D support. Neither
  handles dynamic glow color well.
- Verdict: **Not recommended for the fruit itself.** Good for
  supplementary UI animations (confirmation slider, onboarding
  transitions, etc.)

**Pre-rendered video loop:**
- Pros: Highest visual quality, simple playback
- Cons: Can't dynamically change glow color/intensity. Large files for
  all state × flavor combinations. No interactivity.
- Verdict: **Not recommended.**

### 13.4 Implementation Architecture

```
┌─────────────────────────────────────────┐
│           CitrusBubbleView              │
│         (custom SurfaceView)            │
├─────────────────────────────────────────┤
│                                         │
│  ┌──────────────────────────────────┐   │
│  │   SpriteAnimator                 │   │
│  │   - frames[]: Bitmap (per flavor)│   │
│  │   - currentFrame: Int            │   │
│  │   - rpm: Float (state-driven)    │   │
│  └──────────────────────────────────┘   │
│                                         │
│  ┌──────────────────────────────────┐   │
│  │   GlowShader (GLSL fragment)     │   │
│  │   - intensity: Float [0..1]      │   │
│  │   - color: vec3                  │   │
│  │   - pulsePhase: Float            │   │
│  └──────────────────────────────────┘   │
│                                         │
│  ┌──────────────────────────────────┐   │
│  │   PhysicsAnimator                │   │
│  │   - shake(amplitude, duration)   │   │
│  │   - wobble(dampingRatio)         │   │
│  │   - swell(targetScale, duration) │   │
│  └──────────────────────────────────┘   │
│                                         │
│  ┌──────────────────────────────────┐   │
│  │   StateManager                   │   │
│  │   - currentState: BubbleState    │   │
│  │   - transition(to: BubbleState)  │   │
│  │   (orchestrates all animators)   │   │
│  └──────────────────────────────────┘   │
│                                         │
└─────────────────────────────────────────┘
```

### 13.5 Performance Budget

| Resource | Budget |
|----------|--------|
| Memory per flavor | ≤ 2MB (compressed sprite sheet) |
| GPU usage (idle) | < 1% (static frame + minimal glow shader) |
| GPU usage (active) | < 5% (frame cycling + glow animation) |
| Battery impact | Negligible — idle fruit should not measurably impact battery life |
| Frame rate | 60fps for all shader-driven animations; sprite cycling at variable rate per state |

---

## 14. Onboarding Flow (UI-Relevant Decisions)

The following are decided during the mandatory onboarding interview
and affect all UI behavior:

| Decision | Affects |
|----------|---------|
| **Flavor selection** | Fruit color, peel texture, shape, and personality vibe |
| **Personality interview** | Voice character, spoken phrase style |
| **Verbosity level** | Sound cascade tier (verbose/moderate/minimal) |
| **Permission tier** | Which actions require confirmation |
| **Action visualization preference** | Override defaults for action modes |

### 14.1 Onboarding Sequence (UI Moments)

1. **Flavor Picker** — 3D fruits in carousel, spinning, interactive
   (see §9.3)
2. **Personality Interview** — conversational, Citros asks questions in
   text + voice. The fruit is present and alive during this process —
   glowing, spinning, reacting to the user's answers.
3. **Permission Setup** — clear visual explanation of each tier with
   examples. User picks their comfort level.
4. **"Your Citros is ready"** — the fruit does a triumphant glow
   burst + wobble-settle. It's alive. It's yours.

---

## 15. Accessibility Considerations

- All states must be distinguishable without color alone — animation
  patterns differ per state (spin speed, glow pulse rate, motion type)
- Haptic patterns are configurable (intensity, on/off per event)
- Text interface always available (not voice-only)
- Error messages available as text regardless of voice mode setting
- Screen reader compatibility for all interactive elements —
  announces state changes ("Citros is listening", "Citros needs
  confirmation")
- Confirmation state perceivable through multiple channels
  (visual + haptic + optional sound)
- System theme support ensures readability in both light and dark modes
- High-contrast mode: increase glow intensity differential between
  states; add thin ring outline to fruit for edge visibility
- Reduced-motion mode: disable spin and wobble; use glow-only state
  indication

---

## 16. Open Questions

### Horizon 2 — CitrosOS Modes
*Deferred — Joe wants to think more before committing to full-screen
modes (ambient/active/immersive/review). When this lands, consider
upgrading from sprite-sheet to real-time 3D for immersive mode.*

### Marketing Mocks
*Not yet discussed — hero shots, video demo style, device branding.
The 3D fruit renders from Blender could double as marketing assets.*

### Lemon Shape
*The lemon's elongated (ovoid) shape is noted as a "?" in the flavor
catalog. Needs design exploration: does a non-spherical bubble feel
wrong? Or does it add welcome variety? Prototype and test.*

### Flavor Expansion
*Five launch flavors. Consider seasonal or unlockable flavors post-launch
(Yuzu? Kumquat? Pomelo?). The sprite-sheet approach makes adding new
flavors a design-only task — no code changes needed.*

---

*Spec version: 3.0 — 2026-02-10*
*Rethemed for Citros 3D citrus fruit design*
*Based on UX interview with Joe (Topics 1-5, Topic 6 deferred)*
