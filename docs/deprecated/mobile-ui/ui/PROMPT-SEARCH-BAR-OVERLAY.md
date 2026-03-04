# Task: Replace the Bubble Overlay with a Docked Search Bar

## What you're changing

The Fawx overlay system currently has a **Bubble** mode — a floating orb (FAB-style) that hovers over the home screen. Replace it entirely with a **Search Bar** that docks into the exact slot where the Pixel's Google search bar sits at the bottom of the home screen.

The attached images show the Pixel Google search bar for reference. Match its position, proportions, and role — but make it Fawx.

## Why

Fawx runs on a rooted Pixel 10 Pro. As a system-level agent with Accessibility Service and overlay permissions, it can override the home screen search bar widget. Sitting in the Google bar slot feels native — like the phone itself is thinking — rather than a third-party FAB floating on top.

## What the Search Bar replaces

Delete the `BubbleOverlay` composable. The new `SearchBarOverlay` composable takes its place in the overlay mode enum. The other two overlay modes (Panel, Dynamic Island) remain unchanged.

## Search Bar Spec

### Position & Shape
- Docked at the bottom of the home screen, directly above the gesture handle, below the dock row of favourite apps
- Same horizontal position and width as the Pixel's Google search bar
- Full-width pill: horizontal margin 20dp (`g(5)` — where `g(n) = n * 4dp`), height 52dp (`g(13)`), corner radius 28dp (`g(7)`, full pill)

### Background (theme-aware)
- Dark: `rgba(28,28,30,0.88)`, no border, upward shadow `0 -2dp 16dp rgba(0,0,0,0.3)`
- Light: `rgba(242,242,247,0.88)`, `1dp` border using `separator` token, upward shadow `0 -2dp 16dp rgba(0,0,0,0.06)`
- Both themes: 40dp backdrop blur

### Internal Layout
```
[Orb 36dp] — [8dp gap] — [center content, flex] — [mic button 36dp]
```
Left to right:
1. **Orb** — 36dp (`g(9)`), with the standard Directive C glow (`Modifier.shadow(blurRadius = size * 0.45, spreadRadius = size * 0.18, color = glowColor)`). Sits where the Google "G" logo would be.
2. **Center** — Flex, vertically centered, varies by state (see below).
3. **Mic button** — 36dp circle, transparent background, mic icon 16×16dp in `labelTertiary`. Only visible in `idle` and `unread` states. Hidden during `executing`, `completed`, and `failed` to make room for status content.

### Center Content by State

| State | Content |
|-------|---------|
| **Idle** | `"Ask Fawx anything..."` — 14sp, `labelTertiary`. The resting/ambient state. |
| **Executing** | Pulsing dot (6dp circle, `orbColor`, 0.7 opacity) + italic status text (14sp `labelSecondary`, e.g. "Opening calendar...") + **Stop** button (right-aligned, `red` bg, white 12sp bold text, `g(2.5)` radius) |
| **Completed** | Bold status text (14sp/500 `labelPrimary`, e.g. "Reminder set — 3:00 PM") + green check circle (20dp, `green` at 20% alpha bg, 10dp green checkmark SVG) |
| **Failed** | Bold status text (14sp/500, `red`, e.g. "Calendar access denied") + red alert circle (20dp, `red` at 18% alpha bg, "!" in 11sp/700 `red`) |
| **Unread** | `"Ask Fawx anything..."` placeholder + red badge pill (min 20dp wide, `red` bg, "2" in 11sp/700 white) + small red dot on the Orb itself (10dp, top-right, absolute positioned) |

### Orb State Variants
- Default: standard `orbColor` / `orbInner` / `orbGlow` per the active flavor
- Failed: orb turns `red`, inner becomes `rgba(255,255,255,0.2)`, glow becomes `rgba(255,69,58,0.15)`
- Unread: standard orb + tiny red dot indicator at top-right

### Transitions
- All property changes: 250ms ease
- The bar itself does NOT animate in size (it's always the same pill). Only the center content cross-fades between states.

### Tap Behavior
Tapping the search bar expands it into the slide-up Panel overlay.

## Tokens Reference

Use the existing Directive C theme tokens. Do not invent new ones:

**Surfaces (dark):** `#000000` → `#1C1C1E` → `#2C2C2E` → `#3A3A3C` → `#48484A`
**Surfaces (light):** `#FFFFFF` → `#F2F2F7` → `#E5E5EA` → `#D1D1D6` → `#C7C7CC`
**Labels (dark):** Primary `#FFFFFF`, Secondary `rgba(235,235,245,0.60)`, Tertiary `rgba(235,235,245,0.30)`
**Labels (light):** Primary `#000000`, Secondary `rgba(60,60,67,0.60)`, Tertiary `rgba(60,60,67,0.30)`
**Separator (dark):** `rgba(84,84,88,0.36)` / **(light):** `rgba(60,60,67,0.12)`
**Semantic:** green `#30D158`/`#34C759`, red `#FF453A`/`#FF3B30`

**Flavor glow formula:** `rgba(flavor.primary, 0.15)` — blur `size * 0.45`, spread `size * 0.18`
**No-flavor glow:** dark `rgba(255,255,255,0.06)` / light `rgba(0,0,0,0.04)`

## Implementation Rules

1. **No Material Design.** No `MaterialTheme`, `Surface()`, or Material icons. Build from `Box`, `Row`, `Column`.
2. **4dp grid.** Every dimension must be expressible as `g(n)`.
3. **Theme-aware.** Dark and light must both work. No hardcoded colors outside token definitions.
4. **The Orb always glows.** Even at 36dp. No exceptions.
5. **`none` flavor must work.** White orb (dark) / black orb (light), neutral glow, no crashes.
6. **Custom SVG icons only.** The mic icon is a stroke-only SVG, 1.5dp stroke, round cap+join.
7. **Overlay mode enum.** Update `SearchBar` / `Panel` / `DynamicIsland` — remove `Bubble` entirely.
8. **Settings reference.** If there is a "Default Overlay" picker in settings, update the options from `["Mini Chat", "Bubble", "Dynamic Island"]` to `["Search Bar", "Panel", "Dynamic Island"]`.
