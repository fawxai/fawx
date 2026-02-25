# Citros MVP UI Design Brief

> Feed this to Claude Cowork to generate interactive Compose UI prototypes for the complete MVP.

## What is Citros?

Citros is an AI phone agent for Android. Users chat with an AI that can physically interact with their phone — tap buttons, type text, launch apps, read screens, navigate UIs. Think of it as an AI assistant that doesn't just answer questions but actually *does things* on your phone.

**Brand:** 3D citrus fruit. The app icon is a whole citrus fruit (not a slice) that glows from within. Users pick a "flavor" during onboarding (Lime, Tangerine, Lemon, Blood Orange, Grapefruit) which sets their color theme.

**Vibe:** Premium but approachable. Warm, organic, alive. Not enterprise. Not flat. Not boring.

---

## Current State

The current UI is functional but ugly — a basic dark theme with a single chat screen, a crude sign-in prompt, and no settings. Everything lives in one 1800-line `ChatActivity.kt`. We need to redesign the full MVP experience.

### Current theme (for reference):
```kotlin
primary = Color(0xFFFF9800)      // Tangerine
secondary = Color(0xFF8BC34A)    // Lime
background = Color(0xFF121212)
surface = Color(0xFF1E1E1E)
```

---

## Screens to Design

Design all screens below as a connected interactive prototype with realistic data. Dark mode primary, light mode supported.

## Canonical Interaction Decisions (must follow)

- **Single key-entry pattern:** onboarding API-key step and Settings API-key management use the same Add Key modal bottom sheet from `wallet-ui-design-brief.md`.
- **Model source of truth:** Settings → Models is canonical. Quick Switcher offers fast, limited model switches only.
- **Key health semantics:** key health is always shown as icon + label + color (`Valid`, `Checking`, `Invalid`) across wallet, quick switcher, and chat affordances.
- **Confirmation semantics:** risky actions support both swipe-to-confirm and explicit button alternatives; timeout defaults to deny.
- **Onboarding progression:** users can defer non-critical setup (permissions, advanced personalization) and still reach chat, with persistent follow-up prompts.

---

### 1. Onboarding Flow (First Launch Only)

**1a. Welcome Screen**
- Full-bleed 3D citrus fruit hero image (centered, glowing)
- "AI that uses your phone" tagline
- Subtle particle/glow animation around the fruit
- Single "Get Started" button (pill-shaped, warm gradient)
- No navigation chrome — immersive

**1b. Choose Your Flavor**
- 5 citrus fruits in a horizontal carousel or grid:
  - 🍋 **Lemon** — warm yellow `#FFD600`
  - 🍊 **Tangerine** — orange `#FF8C00`
  - 🟢 **Lime** — green `#7CB342`
  - 🔴 **Blood Orange** — deep red-orange `#D84315`
  - 🟡 **Grapefruit** — pink-coral `#E91E63`
- Each fruit is a 3D-rendered sphere with realistic peel texture
- Tapping a fruit applies that color theme in real-time (background, accents shift)
- "This is your Citros" confirmation text
- Continue button

**1c. Personality Interview**
- Conversational cards (not a form) — the AI asks questions:
  - "How should I talk to you?" → Casual / Professional / Playful
  - "How much should I explain?" → Brief / Balanced / Detailed
  - "What's your comfort level?" → Ask before everything / Ask for risky stuff / Full autonomy
- Each question is a chat-style bubble from Citros, with tap-to-select option chips below
- The selected option fades into a "your choice" style, then next question slides in
- Progress dots at bottom
- Show global progress ("Step X of 6") and allow Back navigation after the welcome screen

**1d. API Key Setup (BYO)**
- Use the same Add Key modal bottom sheet pattern defined in `wallet-ui-design-brief.md`
- Clean card with provider chips: Anthropic / OpenAI / OpenRouter
- Paste field with auto-detection (chip lights up as user pastes)
- "Test Connection" → animated spinner → checkmark or error shake
- Helper text: "Get a key from [provider]" with Chrome Custom Tab link
- Citros Base/Super tiers show "Coming Soon — Join Waitlist" with email capture

**1e. Permissions**
- Two permission cards stacked vertically:
  - **Accessibility Service** — "Let Citros see and interact with your screen"
    - Phone illustration showing Citros tapping a button
    - "Enable" button → opens Android settings
  - **Overlay Permission** — "Show Citros as a floating bubble"
    - Bubble preview illustration
    - "Enable" button
- Each card has a green checkmark when granted, gray when pending
- "Skip for now" text link at bottom with explicit copy about limitations and where to finish setup later

**1f. Ready Screen**
- Citrus fruit animation (gentle bounce/pulse in user's chosen color)
- "You're all set!" heading
- Brief summary: "Provider: Anthropic | Model: Sonnet 4.5 | Trust: Balanced"
- "Start Chatting" button with satisfying haptic-style animation

---

### 2. Main Chat Screen

This is where users spend 90% of their time. It needs to be beautiful.

**Top Bar:**
- App name "Citros" (left)
- Active provider icon + model name as a tappable chip (center-right) — opens quick switcher
- Settings gear icon (right)
- Subtle gradient or blur behind the top bar

**Chat Messages:**
- **User messages:** Right-aligned bubbles on `Flavor Tint` surfaces with a `Flavor Primary` accent border; avoid placing small text directly on raw `Flavor Primary` fills
- **AI messages:** Left-aligned bubbles in elevated surface color with subtle border
- **Action messages** (when Citros does something on the phone):
  - Different visual treatment — maybe a card with an icon (🤖 + action description)
  - "Opened Settings app" / "Tapped 'Wi-Fi' toggle" 
  - Subtle animation or icon indicating physical phone interaction
- **Thinking state:** Animated dots or a small pulsing citrus icon, not just "Thinking..."
- **Streaming text:** Appears word-by-word with a gentle fade-in, not jarring pop-in
- Timestamps: subtle, only shown on first message in a time cluster
- Generous spacing between message groups

**Message Input:**
- Rounded text field with ghost text "Message Citros..."
- Send button (citrus-colored filled circle with arrow)
- Voice input button (microphone icon, left of text field)
- Text field expands vertically for multi-line input
- Subtle elevation above the chat area

**Empty State (no messages yet):**
- Centered citrus fruit (small, gentle pulse)
- "What can I help you with?" 
- 3-4 suggestion chips: "Set a timer", "Open my email", "What's on my calendar?", "Take a screenshot"
- Chips use outline style in user's flavor color

**Accessibility Banner (when not enabled):**
- Compact banner at top (below toolbar)
- "Enable phone control to let Citros interact with your screen"
- "Enable" button + dismiss X
- Non-intrusive but visible

---

### 3. Quick Switcher (Bottom Sheet)

Triggered by tapping the provider chip in the chat toolbar.

- **Compact bottom sheet** (half-screen max)
- **Active Key** section: shows current key with provider icon, label, and health badge (`Valid`, `Checking`, `Invalid`)
- **Other Keys** list: tap to switch (instant, no confirmation)
- **Model Section:** Two rows of chips
  - Chat Model: Sonnet 4.5 | Opus 4.5 | Haiku 4.5 (filtered for active provider)
  - Action Model: Haiku 4.5 | Sonnet 4.5
  - Active selections filled, others outlined
- Drag handle at top
- "Manage Keys" text link → opens full Settings

---

### 4. Settings Screen

Accessible via gear icon in chat toolbar.

**4a. Settings Main**
- User's citrus fruit avatar at top (small, with flavor name)
- Settings sections as cards:
  - **🔑 API Keys** → Key Wallet screen (see `wallet-ui-design-brief.md`)
  - **🧠 Models** → Chat Model + Action Model selection
  - **🛡️ Trust Level** → Permission tier selector
  - **🎨 Appearance** → Flavor picker, theme toggle (dark/light/system)
  - **🔊 Sound & Haptics** → Voice, sounds, haptic feedback toggles
  - **📱 Phone Control** → Accessibility service status, overlay permission
  - **ℹ️ About** → Version, licenses, "Made with 🍊"
- Clean list with icons, no clutter

**4b. Key Wallet** (see separate `wallet-ui-design-brief.md` — already written)

**4c. Models Screen**
- Provider name + icon at top (from active key)
- **Chat Model** section:
  - List of available models as selectable cards
  - Each card: model name, tier badge (💎/🧠/⚡), brief description
  - Active model highlighted
- **Action Model** section (same layout)
- "What's the difference?" expandable explainer:
  - "Chat model handles your conversations — pick the smartest one you can afford"
  - "Action model runs phone interactions — pick the fastest one for snappy responses"

**4d. Trust Level Screen**
- Four cards with radio selection:
  - 🔒 **Full Control** — "Asks before everything"
  - 🛡️ **Balanced** (recommended) — "Asks before messages, emails, and deletions"
  - ⚡ **Relaxed** — "Asks before purchases and deletions only"
  - 🚀 **Autonomous** — "Never asks (expert mode)"
- Current selection has accent border + checkmark
- Brief warning text on Autonomous: "Citros will act without asking. You can always interrupt by holding the screen."

**4e. Appearance**
- Flavor carousel (same as onboarding but horizontal scroll)
- Theme: Dark / Light / System (segmented control)
- Preview card showing how chat bubbles look with current settings

---

### 5. Action Confirmation Dialog

When Citros needs permission to do something (based on trust level):

- **Overlay card** that slides up from bottom (not a system dialog)
- Color-coded border by risk:
  - 🟢 Green — low risk ("Open Settings")
  - 🟡 Yellow — medium risk ("Send a message to Mom")
  - 🔴 Red — high risk ("Delete all photos", "Purchase $49.99 item")
- Content:
  - Citros icon + "Citros wants to:" heading
  - Action description in plain language
  - App icon if relevant
- **Swipe-to-confirm slider** at bottom (intentional gesture, prevents accidental approval)
  - Slider track colored by risk level
  - "Slide to approve" text
- Explicit `Approve` and `Deny` buttons for accessibility services and non-gesture input
- 60-second timeout with countdown indicator; timeout defaults to deny and posts a visible "Request timed out" status

---

### 6. Error States

**Connection Error:**
- Inline banner below toolbar (not a dialog)
- Red-tinted, with retry button
- "Couldn't reach [Provider]. Check your connection."

**Invalid API Key:**
- Chat bubble from Citros: "I can't connect with this API key. It might be expired or invalid."
- Inline "Update Key" button within the bubble
- Key health badge switches to `Invalid` (icon + label + red accent) in quick switcher

**Rate Limited:**
- Friendly message: "The AI provider is busy. Trying again in X seconds..."
- Auto-retry with countdown
- Not raw JSON (current bug)

---

## Design System

### Color Palette

| Token | Dark Mode | Light Mode | Usage |
|-------|-----------|------------|-------|
| Background | `#0F0F0F` | `#FAFAFA` | App background |
| Surface | `#1A1A1A` | `#FFFFFF` | Cards, sheets |
| Surface Elevated | `#242424` | `#F5F5F5` | Raised cards |
| On Surface | `#E8E8E8` | `#1A1A1A` | Primary text |
| On Surface Dim | `#888888` | `#757575` | Secondary text |
| Outline | `#333333` | `#E0E0E0` | Borders, dividers |

**Flavor accents (user-selected):**

| Flavor | Primary | Glow | Tint |
|--------|---------|------|------|
| Lemon | `#FFD600` | `#FFF9C4` | `#332B00` |
| Tangerine | `#FF8C00` | `#FFE0B2` | `#331C00` |
| Lime | `#7CB342` | `#DCEDC8` | `#1A2E0D` |
| Blood Orange | `#D84315` | `#FFCCBC` | `#2E0D04` |
| Grapefruit | `#E91E63` | `#F8BBD0` | `#2E0413` |

**Flavor usage rules (readability first):**
- Use `Flavor Tint` as the default fill for chat bubbles/chips and `On Surface` for text
- Use `Flavor Primary` for borders, selected states, icons, and emphasis accents
- If a filled flavored surface is used for text, compute `onFlavor` dynamically and enforce AA contrast

**Provider accents:**

| Provider | Color | Icon |
|----------|-------|------|
| Anthropic | `#D97757` | Coral shield |
| OpenAI | `#10A37F` | Green circle |
| OpenRouter | `#6366F1` | Indigo diamond |

### Typography
- **Display:** Bold, slightly rounded (welcoming headings)
- **Body:** Clean, high readability at small sizes
- **Code/Technical:** Monospace for API keys, model IDs
- Material 3 type scale throughout

### Shape
- Cards: 16dp rounded corners
- Buttons: Pill/fully rounded (28dp)
- Bottom sheets: 28dp top corners
- Chat bubbles: 16dp with 4dp on the "tail" corner
- Input fields: 24dp rounded

### Motion
- Screen transitions: Shared element transitions where possible
- Bottom sheets: Spring animation (slightly bouncy)
- Message appear: Fade in + slight slide up
- Thinking indicator: Pulsing glow or animated dots
- Confirmation slider: Smooth drag with haptic detents
- Flavor selection: Color theme cross-fades in real-time
- Error states: Gentle shake animation

### Iconography
- Material Symbols (rounded variant) for system icons
- Custom fruit illustrations for flavors
- Provider logos for key wallet

### UX Guardrails (Non-Negotiable)
- WCAG AA contrast minimums: 4.5:1 (normal text), 3:1 (large text/icons)
- Never encode risk/health/error with color alone; always pair with text or icon labels
- Minimum touch targets are 48dp, including chips and icon-only buttons
- Support larger font scaling without clipped content in chips, dialogs, and bottom sheets
- Respect reduced-motion preferences by turning off non-essential pulsing, shaking, and bounce effects
- Loading and retry states must be explicit and reversible (user can retry, cancel, or continue safely)

---

## Technical Context

- **Framework:** Jetpack Compose with Material 3 / Material You
- **Module:** `:chat` is the main APK
- **Navigation:** Single-activity with Compose navigation (NavHost)
- **State:** ViewModel + Compose state (already implemented)
- **Min SDK:** 26 (Android 8.0)
- **Cross-screen UI state needed for consistency:**
  ```kotlin
  enum class KeyHealth { VALID, CHECKING, INVALID, UNKNOWN }

  data class KeyHealthUiState(
      val keyId: String,
      val health: KeyHealth,
      val healthLabel: String,   // "Valid", "Checking", "Invalid"
      val lastValidatedAt: Long?,
      val errorMessage: String?
  )
  ```

## Sample Data for Prototypes

**User:** "Hey, can you check my email and tell me if anything's urgent?"

**Citros (thinking):** [pulsing animation]

**Citros:** "I'll open Gmail and check your inbox. One moment..."

**Citros (action):** 📱 Opened Gmail
**Citros (action):** 👁️ Reading inbox — 12 unread messages

**Citros:** "You have 12 unread emails. Three look urgent:
1. **Meeting moved to 3pm** — from Sarah (30 min ago)
2. **Invoice overdue** — from Quickbooks (2 hours ago)  
3. **Flight confirmation** — from Delta (needs action by tonight)

Want me to reply to any of these?"

---

## What to Produce

Create an interactive artifact that shows:
1. **Complete onboarding flow** (welcome → flavor → personality → API key → permissions → ready)
2. **Main chat screen** with sample conversation including action messages
3. **Quick switcher** bottom sheet
4. **Settings** main screen + key wallet + model picker
5. **Confirmation dialog** in all three risk colors
6. **Error states** inline

Use the Tangerine flavor as default, with 3 sample API keys (Anthropic active, OpenRouter inactive, OpenAI invalid).

Make it production-ready quality — not wireframes. This should look like something you'd screenshot for a Product Hunt launch.

## Acceptance Checklist

- [ ] Onboarding includes step progress and back navigation after welcome
- [ ] Users can defer non-critical onboarding steps and still reach chat
- [ ] API key setup uses the same Add Key modal bottom sheet pattern as wallet
- [ ] Settings -> Models is the canonical model configuration screen
- [ ] Quick switcher supports fast switching but stays scoped to lightweight actions
- [ ] Key health appears as icon + label + color across chat, switcher, and wallet surfaces
- [ ] Chat bubbles and flavored UI elements follow flavor usage rules and maintain readable contrast
- [ ] Confirmation flow supports both swipe-to-confirm and explicit button alternatives
- [ ] Confirmation timeout behavior is explicit and defaults to deny with visible feedback
- [ ] Error states are human-readable, actionable, and never show raw JSON payloads
- [ ] All tap targets meet 48dp minimum and layouts hold up under large font scaling
- [ ] Motion honors reduced-motion settings and removes non-essential effects when enabled
