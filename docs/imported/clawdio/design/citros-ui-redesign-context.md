# Citros Android App — Full UI Redesign Context Bundle

Use this to redesign the entire Citros app UI in Jetpack Compose. The goal is to match the aesthetic of the Citros landing page (https://citros.ai) — dark, orange/amber glowing sphere, elegant and minimal.

---

## What This App Is

Citros is an AI phone agent — an Android app that can use your phone for you. It has:
- **Onboarding flow** (Welcome → Flavor → Conversation Style → Paywall → API Key → Permissions → Ready → Chat)
- **Main chat screen** (message bubbles, suggestion chips, quick switcher, input bar)
- **Overlay system** (mini-chat card, floating bubble, full-app mode — shown over other apps during phone control)
- **Settings hub** with sub-screens (API Keys, Models, Trust Level, Phone Control, Sound & Haptics, Appearance, About)

---

## Design Direction

**Reference:** https://citros.ai — study this page and use its color scheme, typography feel, and visual language as the source of truth for the redesign.

**Target aesthetic:**
- Match the citros.ai landing page colors exactly — background, text, muted text, accent, card backgrounds, borders
- Glowing sphere hero graphic with subtle animation
- Clean, minimal, premium feel
- Large rounded pill buttons with accent fill
- Generous spacing, vertically centered content where appropriate
- Cards with subtle borders and translucent backgrounds

**Do NOT invent a new color scheme.** Extract colors from the citros.ai landing page and translate them to Compose.

---

## Flavor System

The app has 5 "flavors" (color themes). All UI should be flavor-aware — use `flavor.primary`, `flavor.glow`, `flavor.tint` instead of hardcoded accent colors where possible.

```kotlin
enum class CitrosFlavor(
    val storageValue: String,
    val displayName: String,
    val primary: Color,    // Main accent color
    val glow: Color,       // Light/bright variant
    val tint: Color        // Dark variant (used for text on primary)
) {
    LEMON(   primary = Color(0xFFFFD600), glow = Color(0xFFFFF9C4), tint = Color(0xFF332B00)),
    TANGERINE(primary = Color(0xFFFF8C00), glow = Color(0xFFFFE0B2), tint = Color(0xFF331C00)),  // Default
    LIME(    primary = Color(0xFF7CB342), glow = Color(0xFFDCEDC8), tint = Color(0xFF1A2E0D)),
    BLOOD_ORANGE(primary = Color(0xFFD84315), glow = Color(0xFFFFCCBC), tint = Color(0xFF2E0D04)),
    GRAPEFRUIT(primary = Color(0xFFE91E63), glow = Color(0xFFF8BBD0), tint = Color(0xFF2E0413)),
}
```

---

## Current Component Library

These are the existing reusable Composables. You can modify them or create new ones.

### CitrosHeroSphere (animated hero graphic)
```kotlin
@Composable
internal fun CitrosHeroSphere(
    flavor: CitrosFlavor,
    size: Dp = 200.dp,
    modifier: Modifier = Modifier
)
// Draws: radial gradient sphere + pulsing glow ring + 3 orbiting dots
// Uses Compose Canvas + infiniteTransition animations
```

### CitrusHeroBadge (small static badge)
```kotlin
@Composable
internal fun CitrusHeroBadge(flavor: CitrosFlavor, size: Int = 68)
// Simple radial gradient circle — used in flavor selection cards, settings hub, chat empty state, overlay bubble
```

### CitrusPrimaryButton
```kotlin
@Composable
internal fun CitrusPrimaryButton(
    text: String, onClick: () -> Unit, enabled: Boolean = true,
    modifier: Modifier = Modifier, flavor: CitrosFlavor = CitrosFlavor.TANGERINE
)
// Rounded pill button (999.dp radius), flavor.primary bg, flavor.tint text
```

### CitrosStepHeader
```kotlin
@Composable
internal fun CitrosStepHeader(
    title: String, stepIndex: Int, totalSteps: Int,
    onBack: (() -> Unit)? = null, modifier: Modifier = Modifier
)
// Shows: [Back] Title [stepIndex/totalSteps] + progress bar (row of colored/gray boxes)
```

### PersonalityOptionChip
```kotlin
@Composable
internal fun PersonalityOptionChip(text: String, selected: Boolean, onClick: () -> Unit)
// Rounded pill chip, selected = primary border + tinted bg
```

### FlavorOptionCard
```kotlin
@Composable
internal fun FlavorOptionCard(
    flavor: CitrosFlavor, selected: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier
)
// Full-width card with CitrusHeroBadge + flavor name + "Selected" label
```

### PlanCard
```kotlin
@Composable
internal fun PlanCard(plan: CitrosPlanSpec, onSelect: () -> Unit, modifier: Modifier = Modifier, testTag: String? = null)
// Plan selection card with title, subtitle, details, CTA button. Recommended variant has accent border.
```

### PortedMessageBubble
```kotlin
@Composable
internal fun PortedMessageBubble(message: Message, flavor: CitrosFlavor)
// Chat message bubble. User messages right-aligned with flavor.tint bg.
// Assistant messages left-aligned. Steer messages have dashed border + lower alpha.
// Action messages (🤖/📱/👁 prefixed) get secondaryContainer bg.
```

### PortedLoadingIndicator
```kotlin
@Composable
internal fun PortedLoadingIndicator(flavor: CitrosFlavor = CitrosFlavor.TANGERINE, label: String = "Thinking")
// 3 animated dots + label, left-aligned
```

### ChatEmptyState
```kotlin
@Composable
internal fun ChatEmptyState(flavor: CitrosFlavor, onSuggestion: (String) -> Unit)
// Hero badge (56dp) + "Hey there! What can I help you with?" + suggestion AssistChips
// (Set a timer, Open my email, Calendar, Screenshot)
```

### ProviderModelChip
```kotlin
@Composable
internal fun ProviderModelChip(walletState: WalletState, onClick: () -> Unit, modifier: Modifier = Modifier)
// Small pill chip showing current provider icon + short model name. Appears in chat top bar.
```

### QuickSwitcherSheet
```kotlin
@Composable
internal fun QuickSwitcherSheet(...)
// ModalBottomSheet for switching active API key and chat/action models.
// Shows active key, other keys, chat model chips, action model chips.
```

---

## Screen Inventory — Full App

### 1. ONBOARDING FLOW

#### 1a. WELCOME
- Hero sphere (200dp, animated)
- "Citros" title
- "AI that uses your phone" subtitle
- Page indicator dots
- "Get Started" button

#### 1b. FLAVOR (Choose Your Flavor)
- Step header (1/7)
- Description text
- List of 5 FlavorOptionCards

#### 1c. CONVERSATION_STYLE
- Step header (2/7)
- "How should I talk to you?" → Casual / Professional / Playful chips
- "How much should I explain?" → Brief / Balanced / Detailed chips
- "Comfort level" → Ask before everything / Ask for risky stuff / Full autonomy chips
- Continue button

#### 1d. PAYWALL (Choose Your Plan)
- Step header (3/7)
- 3 PlanCards: Free Trial, Bring Your Own Key (recommended), Citros Pro (coming soon)

#### 1e. API_KEY (Enter API Key)
- Step header (4/7)
- Provider selection (Anthropic/OpenAI)
- API key text input field
- Validation status
- Link to get API key
- Continue/Skip buttons

#### 1f. PERMISSIONS
- Step header (5/7)
- Permission cards for Accessibility, Notifications, Overlay
- Each shows status (Granted/Not granted) with action button
- Continue button

#### 1g. READY (You're all set!)
- Hero badge
- "You're all set!" title
- "Your AI phone agent is ready to go" subtitle
- Capability list (Use your phone, Remember context, Search the web, Learn your preferences)
- "Start Chatting" button

### 2. MAIN CHAT SCREEN (`ChatScreen`)
- **Top bar:** ProviderModelChip (tappable → QuickSwitcherSheet), settings gear icon
- **Empty state:** ChatEmptyState (hero badge + greeting + suggestion chips)
- **Message list:** LazyColumn of PortedMessageBubble items
- **Loading indicator:** PortedLoadingIndicator ("Thinking" dots)
- **Input bar:** TextField + Send IconButton (uses flavor.primary tint)
- **Overlay button:** Navigate to overlay preview

### 3. OVERLAY SYSTEM (`OverlayPreviewScreen` / `OverlayPortedScreen`)

Three surface modes with a control panel to switch between them:

#### 3a. Mini-Chat
- Floating card over other apps (bottom-anchored)
- Header: CitrusHeroBadge + status label + "Full"/"Bubble" mode buttons
- Lines: scrollable log (user messages, system messages, queued messages)
- Step counter AssistChip
- Contextual banner: Stopped → Resume button, Failed → Retry button
- Input row: TextField + Send + Stop button (during execution)

#### 3b. Bubble
- Small floating circle (58dp) with CitrusHeroBadge inside
- CircularProgressIndicator ring during execution
- Unread badge (top-right)
- Long-press → quick actions menu (Stop, Expand, Dismiss)

#### 3c. Full App
- Full-screen overlay with chat header, status card, message log, input bar
- Status card shows run state + current step + Return/Stop buttons

#### Overlay Colors (current hardcoded)
```kotlin
object OverlayColors {
    val AppChrome = Color(0xFF101423)
    val PreviewBackground = Color(0xFF121727)
    val FakePhoneBase = Color(0xFF1A1A2E)
    val FakePhoneBar = Color(0xFF16213E)
    val FakePhoneSurface = Color(0xFF0F3460)
    // ... etc
}
```
These should be updated to match the citros.ai palette.

### 4. SETTINGS HUB (`SettingsHubScreen`)
- Top bar with "Settings" title and "Back" text button
- Profile card: CitrusHeroBadge (42dp) + "Citros" + active key/model info
- Navigation cards (7): API Keys, Models, Sound & Haptics, Trust Level, Phone Control, Appearance, About
- Each card: icon in tinted box + title + subtitle + chevron

### 5. SETTINGS SUB-SCREENS

#### 5a. API Keys (`SettingsScreen`)
- Scaffold with TopAppBar + FAB (Add key)
- Empty state: emoji + "Add your first key"
- Key list: SwipeToDismissBox cards with provider icon, label, masked key, health dot (green/red/yellow), delete button
- Active key has accent border
- Model selection dropdowns (chat + action)
- AddKeyBottomSheet: provider chips, key input (password masked), label, test connection button

#### 5b. Models (`ModelsSettingsScreen`)
- ModelSelectionSection (shared component): chat model dropdown + action model dropdown
- "No API Key Active" state with icon

#### 5c. Trust Level (`TrustSettingsScreen`)
- 3 options as Surface cards: "Ask before everything", "Ask for risky stuff", "Full autonomy"
- Selected card gets primary tint background
- Each has title + description text

#### 5d. Appearance (`AppearanceSettingsScreen`)
- Flavor section: 5 FlavorOptionCards
- Auto-clear section: timeout pills (row of Surface buttons)
- Theme section: dark/light/system pills

#### 5e. Phone Control (`PhoneControlSettingsScreen`)
- Permission cards for Accessibility Service and Display Over Other Apps
- Each shows granted/not-granted status with "Open Settings" action
- Default Overlay Mode selector: Mini Chat / Bubble pills

#### 5f. Sound & Haptics (`SoundSettingsScreen`)
- Placeholder: icon + "Coming soon" text

#### 5g. About (`AboutSettingsScreen`)
- "Citros" heading + tagline
- Version info card (Version, Runtime, UI framework, Min SDK)
- "Made with citrus intent." footer

---

## Constraints

- **Output Jetpack Compose (Kotlin)** — this is an Android app, not web
- **Material3** — use `MaterialTheme.colorScheme.*` and `MaterialTheme.typography.*`
- **Flavor-aware** — use `flavor.primary`, `flavor.glow`, `flavor.tint` for accent colors
- **Dark mode only** for now (the default)
- **Keep the same navigation structure** — don't add or remove screens/routes
- **Keep component signatures compatible** — modify internals freely but keep the function names/parameters
- **All text must have explicit `color`** — don't rely on default text color (it doesn't always pick up `onBackground` in our setup)
- **Root containers need explicit `background(MaterialTheme.colorScheme.background)`** — the window background is XML-based and may not match
- **Use the citros.ai landing page as the color reference** — extract its palette and translate to Compose color values. Do not invent new colors.

---

## What I Want You to Redesign

Make the **entire app** look premium and cohesive, matching the citros.ai landing page vibe. Every screen should feel like it belongs to the same brand.

### Onboarding
1. **Welcome screen** — Hero sphere, typography, button styling, spacing
2. **Step headers** — Progress indicator style, back button, title treatment
3. **Option chips** — Selected/unselected states, border/fill treatments
4. **Cards** — Flavor cards, plan cards — surface colors, borders, shadows
5. **Input fields** — API key input styling
6. **The "Ready" screen** — Make it celebratory/polished

### Main Chat
7. **Chat screen layout** — Top bar, empty state, message list spacing
8. **Message bubbles** — User/assistant/action/steer bubble styling, colors, shapes
9. **Loading indicator** — Dot animation, label styling
10. **Chat empty state** — Hero badge, greeting text, suggestion chips
11. **Input bar** — TextField styling, send button
12. **Quick switcher sheet** — Key cards, model chips, bottom sheet styling

### Overlay
13. **Mini-chat card** — Surface colors, borders, header, log lines, input row
14. **Bubble** — Border/progress ring colors, badge styling
15. **Full-app overlay** — Chrome, status card, message cards, input
16. **Overlay colors** — Replace hardcoded OverlayColors with landing-page-derived palette

### Settings
17. **Settings hub** — Profile card, navigation cards, icon boxes
18. **API Keys screen** — Key cards, health dots, add key sheet, model dropdowns
19. **Trust/Appearance/Phone Control** — Option cards, pill selectors
20. **About screen** — Version card, branding treatment

### Global
21. **Color scheme** — darkColorScheme values derived from citros.ai landing page
22. **Typography** — Font sizes, weights, letter spacing throughout
23. **Spacing & layout** — Consistent padding, card corner radii, surface elevations
24. **Buttons** — Primary and secondary button styling (consistent pill shape, colors)

Output complete Composable functions that I can drop into the codebase.
