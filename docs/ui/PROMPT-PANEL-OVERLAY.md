# Task: Implement the Slide-Up Panel Overlay

## What you're building

A **slide-up panel** that appears from the bottom of the screen when the user taps the Search Bar overlay (or when invoked programmatically). This is the quick-interaction surface — a compact conversation view with a grab handle, header, status/content area, and input bar. It does **not** replace the full Chat screen; it's a lightweight overlay for fast queries and status feedback.

The attached images show the panel in context on the Pixel home screen. Match the proportions and role shown.

## Why

Citros runs on a rooted Pixel 10 Pro with Accessibility Service and overlay permissions. The panel slides up over whatever app is currently active, giving the user a fast way to interact with Citros without leaving their context. It sits between the ambient Search Bar (always visible, status-only) and the full Chat screen (immersive conversation).

## Panel Spec

### Position & Shape
- Anchored to the bottom of the screen, slides up over existing content
- Top corners rounded: `g(4)` = 16dp radius. Bottom corners: 0dp (flush with screen edge)
- Width: full screen width (no horizontal margins)
- Height: intrinsic — grows with content but never exceeds ~40% of screen height in compact mode

### Background (theme-aware)
- Dark: `rgba(28,28,30,0.92)`, no border, backdrop blur 40dp
- Light: `rgba(242,242,247,0.92)`, no border, backdrop blur 40dp
- In Compose: since true backdrop blur requires RenderEffect (API 31+), use a semi-transparent `surface1` as the background color. The mock uses `0.92` alpha.

### Grab Handle
- Centered horizontally at the top of the panel
- Width: `g(9)` = 36dp, height: 5dp, corner radius: 3dp (full pill)
- Color: `surface3`
- Vertical padding: `g(2)` above, `g(1)` below

### Header Row
```
[Orb g(7)] — [g(2.5) gap] — [Title "Citros", flex] — [Expand chip]
```
- Padding: `g(1.5)` top, `g(2.5)` bottom, `g(4)` horizontal
- **Orb:** `g(7)` = 28dp with standard glow. Uses active flavor color/glow/inner tokens.
- **Title:** "Citros" — 15sp, weight 600, tracking -0.2, color `labelPrimary`, flex = 1
- **Expand chip:** Background `surface2`, radius `g(3.5)` = 14dp, padding `g(1.25)` vertical × `g(3)` horizontal, text "Expand" in 13sp `labelSecondary`. Tapping opens the full Chat screen.

### Content Area
Below the header, padded `0` top, `g(2)` bottom, `g(4)` horizontal. Content varies by state:

| State | Content |
|-------|---------|
| **Idle** | `"Ready"` — 14sp, `labelTertiary`, tracking -0.1. The resting state when no task is active. |
| **Executing** | Row: pulsing dot (5dp circle, `orbColor`, 0.6 opacity) + `g(2)` gap + italic status text (14sp, `labelSecondary`, tracking -0.1, e.g. "Opening calendar...") + flex spacer + **Stop** button (`red` bg, white 12sp/600 text, radius `g(3)`, padding `g(1)` × `g(2.5)`) |
| **Completed** | Assistant-style message bubble (`surface2` bg, radius `g(3.5) g(3.5) g(3.5) g(1)` — tail bottom-left, 14sp `labelPrimary`, line-height 20sp, tracking -0.2, padding `g(2)` × `g(3)`, e.g. "Reminder set for 3:00 PM") + below it: success badge (inline-flex row, `green` at `18` hex alpha bg, radius `g(3)`, padding `g(1.25)` × `g(2.5)`, containing 10dp green checkmark SVG + `g(1.5)` gap + "Completed" in 12sp/500 `green`) |
| **Failed** | Error card: `red` at `12` hex alpha bg + `1px solid red` at `22` hex alpha border, radius `g(3)`, padding `g(2)` × `g(3)`, text 14sp `labelPrimary` line-height 20sp (e.g. "Calendar access denied. Tap to open settings.") |

### Input Bar
Below the content area, separated by a **0.5dp `separator`** top border.

- Padding: `g(2)` top, `g(6)` bottom (extra bottom padding for gesture-safe zone), `g(3)` horizontal
- Layout: `[text field flex] [g(2) gap] [send button]`
- **Text field:** `surface2` bg, radius `g(5.5)` = 22dp (full pill), padding `g(2.25)` vertical × `g(3.5)` horizontal. Placeholder "Message" in 14sp `labelTertiary`, tracking -0.2. Input text: 14sp `labelPrimary`.
- **Send button:** `g(8.5)` = 34dp circle, background `sendBg` (= `flavor.primary` or `surface3` for none), icon = up-arrow 14×14dp in `sendIcon` (= `flavor.onPrimary` or `labelSecondary` for none). When text field is empty: background `surface3`, icon `labelQuaternary`.

### Orb State Variants
- Default: standard `orbColor` / `orbInner` / `orbGlow` per the active flavor
- Failed state: the orb does NOT change color in the panel (unlike the search bar). The error card communicates failure.
- The orb always glows, even at `g(7)`.

### Transitions
- Panel slide-up: 250ms ease (translate Y from off-screen to final position)
- Content cross-fade between states: 200ms ease
- The panel height adjusts smoothly if content changes size

### Interaction Behavior
- **Drag down on grab handle:** Dismisses the panel back to the Search Bar
- **Tap "Expand":** Navigates to the full Chat screen
- **Tap outside the panel (on the dimmed backdrop):** Dismisses the panel
- **Backdrop dimming:** When the panel is visible, the area above it dims with `rgba(0,0,0,0.3)` (dark) or `rgba(0,0,0,0.15)` (light)

## Tokens Reference

Use the existing Directive C theme tokens. Do not invent new ones:

**Surfaces (dark):** `#000000` → `#1C1C1E` → `#2C2C2E` → `#3A3A3C` → `#48484A`
**Surfaces (light):** `#FFFFFF` → `#F2F2F7` → `#E5E5EA` → `#D1D1D6` → `#C7C7CC`
**Labels (dark):** Primary `#FFFFFF`, Secondary `rgba(235,235,245,0.60)`, Tertiary `rgba(235,235,245,0.30)`
**Labels (light):** Primary `#000000`, Secondary `rgba(60,60,67,0.60)`, Tertiary `rgba(60,60,67,0.30)`
**Separator (dark):** `rgba(84,84,88,0.36)` / **(light):** `rgba(60,60,67,0.12)`
**Semantic:** green `#30D158`/`#34C759`, red `#FF453A`/`#FF3B30`
**Backdrop dim:** dark `rgba(0,0,0,0.3)` / light `rgba(0,0,0,0.15)` — applied to the area above the panel

**Flavor glow formula:** `rgba(flavor.primary, 0.15)` — blur `size * 0.45`, spread `size * 0.18`
**No-flavor glow:** dark `rgba(255,255,255,0.06)` / light `rgba(0,0,0,0.04)`

**Send button resolution:**
- Flavor set → bg: `flavor.primary`, icon: `flavor.onPrimary`
- Flavor `none` → bg: `surface3` (`#3A3A3C` dark / `#D1D1D6` light), icon: `labelSecondary`
- Empty input → bg: `surface3`, icon: `labelQuaternary`

## Implementation Rules

1. **No Material Design.** No `MaterialTheme`, `Surface()`, `BottomSheet()`, or Material icons. Build from `Box`, `Row`, `Column`, `Modifier.offset`, `Modifier.clip`.
2. **4dp grid.** Every dimension must be expressible as `g(n)`.
3. **Theme-aware.** Dark and light must both work. No hardcoded colors outside token definitions.
4. **The Orb always glows.** Even at 28dp (`g(7)`). No exceptions.
5. **`none` flavor must work.** White orb (dark) / black orb (light), neutral glow, `surface3` send button. No crashes.
6. **Custom SVG icons only.** The send arrow is stroke-only SVG, 2dp stroke, round cap+join. The checkmark in the completed badge is 1.4dp stroke.
7. **0.5dp separator** between content area and input bar. Never 1dp.
8. **Negative letter-spacing.** -0.1 to -0.2sp on all text in the panel.
9. **Overlay mode enum.** This composable is `Panel` in the `SearchBar` / `Panel` / `DynamicIsland` enum.
10. **Gesture dismissal.** The grab handle must support vertical drag-to-dismiss. Use `Modifier.draggable` or `Modifier.pointerInput` with velocity detection — fling down dismisses, slow drag snaps back if < 50% threshold.
