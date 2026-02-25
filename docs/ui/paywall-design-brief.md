# Citros Paywall Screen — Design Brief

> Onboarding step between Personality selection → API Key setup

## Context

This is a mock paywall for the Citros MVP. No real payment processing yet — Base/Super tiers show a "Coming Soon" waitlist. BYO tier is fully functional.

## Canonical Interaction Decisions (must follow)

- This screen is a required onboarding step between Personality and API Key setup, with visible onboarding progress and Back navigation.
- Card tap and CTA tap must trigger the exact same action for each plan (no split behavior).
- Base is visually recommended, but all three options remain equally selectable and dismissible without dark-pattern friction.
- Base/Super never show fake checkout in MVP; they always open the same "Coming Soon" waitlist bottom sheet.
- Skip should route to API Key setup (not a dead end) and persist a valid plan state (`byo`) for continuity.
- Plan state must be explicit in copy and storage (`selected_tier`, trial/waitlist metadata), so onboarding can resume safely after app restarts.

## Brand

- **App:** Citros — AI phone agent with 3D citrus fruit aesthetic
- **Palette:** Lime (#32CD32), Tangerine (#FF6F00), Lemon (#FFD600), Blood Orange (#FF3D00)
- **Background:** Dark (#121212)
- **Tech:** Jetpack Compose, Material 3, dark theme

## Screen Layout (top to bottom)

### 1. Header
- "Choose Your Plan" with a subtle citrus gradient underline
- Keep it clean — no lengthy explanation needed

### 2. Three Plan Cards (vertically stacked)

#### 🔧 Bring Your Own Key — Free
- "Use your own API key from Anthropic, OpenAI, or OpenRouter"
- "All models, no limits — you pay your provider directly"
- **Badge:** "Try free for 2 days" (Lime accent)
- **Subtext:** "Then enter your API key to continue"
- **CTA:** "Select" (outlined, Lime border)

#### 🍊 Citros Base — $9/mo *(highlighted / recommended)*
- "All models included — Haiku, Sonnet, Opus, GPT, Gemini"
- "$5 monthly usage cap • Perfect for getting started"
- **Badge:** "$5 free to start" (Tangerine accent)
- **CTA:** "Join Waitlist" (filled, Tangerine background)
- **State hint:** "Coming Soon" helper text below CTA
- This card should feel elevated — slightly larger, subtle glow or border highlight

#### 🚀 Citros Super — $29/mo
- "All models included — same full catalog, higher caps"
- "$50 monthly usage cap • For power users"
- **Badge:** "$5 free to start" (Blood Orange accent)
- **CTA:** "Join Waitlist" (outlined, Blood Orange border)
- **State hint:** "Coming Soon" helper text below CTA

### 3. Usage Estimate
- Small helper text making caps tangible:
  - "~500 messages/mo on Base • ~5,000 on Super"
- Helps users understand what the dollar caps mean in practice

### 4. Fine Print
- "Cancel anytime • Usage resets monthly • All plans include phone control"

### 5. Skip Link
- "I'll decide later →" (muted text, bottom of screen)
- Skipping routes directly to API key setup (no blocked or confusing detour)

## Behavior

| Action | Result |
|--------|--------|
| Tap BYO card or CTA | Persist `selected_tier=byo`, start 2-day trial metadata, proceed to API key setup. |
| Tap Base/Super card or CTA | Open "Coming Soon" bottom sheet with email field + "Notify Me"; no billing UI shown. |
| Tap "I'll decide later" | Persist `selected_tier=byo`, then route to API key setup with no blocked path. |

## Interaction Details

- **Card press animation:** Slight scale-down (0.97) + elevation change on press
- **Highlighted card:** Base card has 2dp extra elevation, faint Tangerine border glow
- **Badge animation:** Subtle pulse on first render to draw attention
- **Bottom sheet (Coming Soon):** Standard Material 3 modal bottom sheet with email field + submit button
- **Form behavior:** Email validates inline, keyboard is email-optimized, and submit is disabled until format is valid
- **Dismiss behavior:** Sheet has clear close affordance and can be dismissed without losing current onboarding progress

## Accessibility & UX Guardrails

- WCAG AA contrast minimums: 4.5:1 for normal text, 3:1 for large text/icons
- Never encode recommendation/status with color alone; pair color with icon/label text (`Recommended`, `Coming Soon`, etc.)
- Minimum touch target size: 48dp for cards, CTAs, close icons, and text links
- Motion must respect reduced-motion settings (disable pulse/glow/scale where non-essential)
- Support dynamic type / large font without clipping card copy, prices, badges, and CTA labels
- Skip and Back affordances must remain visible and tappable at all times (no trapped flow)

## Data Storage (Mock)

Persist the decision and onboarding continuity state in SharedPreferences:

```kotlin
// Keys
"selected_tier" → "byo" | "base" | "super"
"trial_start_ms" → System.currentTimeMillis() // BYO 2-day trial
"waitlist_email" → "user@example.com"          // Base/Super waitlist
"waitlist_tier" → "base" | "super"             // Last requested coming-soon tier
"paywall_seen" → true                          // Guard for onboarding resume logic
```

No backend calls — everything local until payment infrastructure exists.

## UI State Contract (Mock)

```kotlin
enum class PaywallTier { BYO, BASE, SUPER }

data class PaywallUiState(
    val selectedTier: PaywallTier?,
    val isComingSoonSheetOpen: Boolean,
    val comingSoonTier: PaywallTier?,
    val waitlistEmail: String,
    val isWaitlistEmailValid: Boolean,
    val isReducedMotion: Boolean
)
```

## Flow Context

```
Welcome → Personality → **Paywall (this screen)** → API Key Setup → Permissions → Ready
```

## Related Docs
- [Onboarding Spec](onboarding-spec.md) — full onboarding flow + tier architecture
- [MVP UI Design Brief](mvp-ui-design-brief.md) — all screens including chat, settings, errors
- [Wallet UI Design Brief](wallet-ui-design-brief.md) — key management screens

## Acceptance Checklist

- [ ] Screen placement matches onboarding flow with visible progress and Back support
- [ ] Card tap and CTA tap behave identically for all plans
- [ ] Base is highlighted as recommended without obscuring BYO/Super choices
- [ ] Base/Super always open the same Coming Soon waitlist sheet (no fake checkout)
- [ ] BYO and Skip both lead to API key setup with persisted valid tier state
- [ ] Waitlist email field validates inline and disables submit until valid
- [ ] Reduced-motion setting disables non-essential pulse/glow/scale animation
- [ ] Color is never the only indicator for recommendation/status/badges
- [ ] Touch targets are >=48dp and content remains usable with larger font sizes
- [ ] SharedPreferences keys are populated consistently for resume/re-entry flows
