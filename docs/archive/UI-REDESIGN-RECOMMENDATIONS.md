# Fawx UI Redesign Recommendations

## Executive Summary

The current Fawx UI has a strong visual identity — the flavor system, glass morphism, and animated orb are distinctive. But for users who prefer **minimal, modern, iOS-aligned design**, the current approach creates tension: 56-particle hero spheres, multi-layer glass gradients, and warm glow shadows on every surface push the aesthetic toward sci-fi dashboard rather than refined tool.

This document proposes a design direction shift: **keep the flavor identity, strip the ornamentation.**

---

## Interactive Mockups

Four React artifacts are included in this repo. Open them to toggle between current and proposed designs:

| File | What it shows |
|------|---------------|
| `chat-redesign-main.jsx` | Full chat screen comparison — top bar, messages, input, empty state |
| `settings-redesign.jsx` | Settings hub: glass card grid vs iOS grouped lists (click into sub-pages) |
| `components-redesign.jsx` | Component library: 3 bubble variants, 4 input bars, tool indicators, model switchers, loading states |
| `overlay-redesign.jsx` | Overlay modes: bubble, mini-chat, and a new Dynamic Island option |

---

## Core Design Principles (Proposed)

1. **Flat over frosted** — Replace `FawxLiquidGlassSurface` with opaque `Surface` composables using iOS system colors (`#1C1C1E`, `#2C2C2E`, `#3A3A3C`)
2. **Flavor as accent, not atmosphere** — The flavor color should appear on user bubbles, send buttons, and interactive elements. It should NOT tint borders, glow, or backdrop particles
3. **iOS system vocabulary** — Grouped table views for settings, SF Pro metrics, 0.5px separators, large title navigation, standard toggle switches
4. **Content density over visual density** — More whitespace, fewer layers, tighter line-height (1.48 instead of 1.5+), negative letter-spacing (-0.2 to -0.5)
5. **Motion with purpose** — Replace infinite particle loops with meaningful transitions: state changes, navigation, feedback

---

## Structural Recommendations

### 1. Chat Screen

**Current issues:**
- Top bar has animated particle backdrop (`FawxFloatingSpriteBackdrop` with 12-84 sprites), animated icon (72s loop), glass model chip, and 3 glass toolbar buttons — too much visual weight for a nav bar
- Message bubbles use 3 gradient layers + border glow + backdrop blur
- Two separate buttons (mic + send) when one adaptive button suffices

**Proposed:**
- Clean nav bar: 32px circular flavor icon (solid, no particles), title + subtitle, single settings button
- User bubbles: solid flavor color (like iMessage blue → your flavor). Assistant bubbles: `#2C2C2E` flat
- Single send button (↑ arrow in flavor circle). Mic can be long-press or appear when input is empty
- Action messages: inline chip with icon + status, not a full bubble

### 2. Settings Hub

**Current issues:**
- 2-column grid of glass cards with hero sphere — treats settings like a dashboard
- Every card has glass gradient, border, blur — slow to render, visually heavy
- Sub-pages use custom scaffold pattern instead of platform navigation

**Proposed:**
- iOS grouped table view (`LazyColumn` with rounded `Surface` sections)
- Large title navigation (34sp, -0.8 letter-spacing) with inline back buttons
- Status badges on rows (e.g., "Active" in green, "Ask for risky" in orange) for at-a-glance info
- Destructive actions (Sign Out) in isolated section per Apple HIG
- Appearance page: visual theme previews (dark/light/system as mini phone cards) instead of text-only pills

### 3. Overlay System

**Current issues:**
- Bubble uses full `FawxHeroShaderSphere` (26-56 particles, 3 rotating rings, 120s animation loop) — massive rendering cost for a 56dp circle floating over other apps
- Mini-chat header has glass orb + "Full"/"Bubble" buttons — two mode-switch buttons feels indecisive

**Proposed three-tier system:**
- **Bubble (revised):** Flat solid-color circle, small badge. No particles, no progress ring SVG. Long-press for context menu
- **Slide-up Panel (revised mini):** iOS sheet with grab handle, frosted background, single "Expand" button. Shows last message + status, not full transcript
- **Dynamic Island (new):** Compact pill at top of screen that expands contextually. Idle → brand pill. Executing → status + stop button. Done → result summary. Failed → error + action. Feels native to iPhone users

### 4. Message Components

**Bubble variants to consider:**
- **Solid (recommended):** User = solid flavor color, assistant = `#2C2C2E`. Clean contrast, no borders needed
- **Tinted:** User = 15% flavor tint + 1px flavor border. Softer than solid, still cleaner than glass
- Keep current glass as an optional "Expressive" mode in Appearance settings for users who want it

**Tool/Action indicators:**
- Replace full action bubbles with inline chips: `[📅 Set reminder ✓]` — icon + label + status
- For multi-step actions, offer an expandable execution timeline (vertical line + step dots)
- Show timing info (200ms, 450ms) to build trust in speed

### 5. Model Switcher

**Current:** Small glass chip in top bar with "▾" dropdown indicator

**Alternatives:**
- **Segmented control** in the top bar (Local | Sonnet | Opus) — good for 2-3 options
- **Contextual menu** (iOS-style popover) with model name, description, and icon — better for details
- **Bottom sheet** with full model cards — good for first-time selection but too heavy for quick switching

---

## Material Design 3 Components Not Currently Used

These M3 components would align well with your design goals:

1. **`NavigationBar`** (bottom) — If you add more top-level destinations beyond Chat and Settings, a bottom nav bar would be more iOS-native than the current toolbar buttons
2. **`ModalBottomSheet` / `BottomSheetScaffold`** — Replace the custom overlay mini-chat with M3's sheet, which handles drag-to-dismiss, peek height, and state transitions
3. **`SegmentedButton`** — Replace the FlowRow of pill buttons in Appearance settings (theme mode, auto-clear) with M3 segmented buttons
4. **`InputChip` / `FilterChip`** — For the suggestion chips in the empty state and tool action tags
5. **`TopAppBarDefaults.pinnedScrollBehavior`** — For the settings sub-pages, a collapsing top app bar that pins on scroll would match iOS large-title behavior
6. **`ListItem`** — M3's `ListItem` with `leadingContent`, `trailingContent`, and `supportingContent` slots maps perfectly to iOS grouped list rows
7. **`LinearProgressIndicator`** — For execution progress, a thin M3 progress bar at the top of the chat (or overlay) would replace the heavy SVG progress ring
8. **`Badge`** — M3 badges for unread counts on the overlay bubble, replacing the custom positioned circle
9. **`RichTooltip`** — For model switcher details on long-press
10. **`DatePicker` / `TimePicker`** — If Fawx ever needs date/time input for reminders, use M3 pickers instead of custom UI

---

## Color System Revision

**Current dark theme:**
- Background: `#050505` / Surface: `#0C0C0C` / SurfaceVariant: `#111111`
- These are too close together (only 7-12 units apart) — insufficient contrast between elevation levels

**Proposed (iOS system colors):**
- Background: `#000000` / Surface: `#1C1C1E` / Elevated: `#2C2C2E` / Tertiary: `#3A3A3C`
- Clear 16-unit steps between levels, matching iOS dark mode exactly
- Label colors: `#FFFFFF` / `rgba(235,235,245,0.6)` / `rgba(235,235,245,0.3)` — proper hierarchy
- Separator: `rgba(84,84,88,0.65)` for prominent, `rgba(84,84,88,0.35)` for subtle

**Flavor usage:**
- Primary: user bubbles, send button, selected states, active indicators
- 15% opacity: tinted backgrounds (action chips, selected rows)
- Never on borders, glows, or atmospheric particles

---

## Animation Budget

**Current:** ~180+ animated elements across chat screen (particles, rings, wobbles, pulses, flickers)

**Proposed budget:**
- **0 infinite background animations** on chat screen
- **1 pulse animation** on the app icon (subtle, 3s cycle, ±2% scale)
- **State transitions only:** enter/exit for bubbles (slide + fade, 250ms), input bar expand (height tween, 200ms), overlay mode switch (shared element transition, 300ms)
- **Onboarding exception:** Keep the hero sphere for the onboarding flow where you WANT to impress — just not in the daily-use chat screen

---

## Typography Tightening

| Element | Current | Proposed |
|---------|---------|----------|
| Nav title | titleLarge | 17sp, -0.3 tracking, weight 600 |
| Large title (settings) | headlineMedium | 34sp, -0.8 tracking, weight 700 |
| Message body | 15sp, 1.5 line-height | 15sp, 1.48 line-height, -0.2 tracking |
| Bubble label | 11sp | 12sp, -0.1 tracking |
| Settings row | bodyLarge | 16sp, -0.2 tracking, weight 400 |
| Section header | labelMedium caps | 13sp, 0.5 tracking, uppercase, secondary color |
| Code blocks | Monospace | 13sp, SF Mono / JetBrains Mono, 1.5 line-height |

---

## Migration Path

You don't have to do this all at once. Suggested order:

1. **Color system** — Swap the 3 near-black backgrounds for the 4-step iOS system. Immediate visual improvement, minimal code change
2. **Settings screens** — Replace glass card grid with grouped table view. Self-contained, doesn't affect chat
3. **Message bubbles** — Switch to solid-color user bubbles. Keep glass as fallback behind a feature flag
4. **Top bar** — Strip particles, simplify to flat icon + text
5. **Input bar** — Single button, remove glass styling
6. **Overlay** — Implement Dynamic Island mode alongside existing bubble/mini
7. **Animation cleanup** — Remove infinite loops from chat screen, keep for onboarding
