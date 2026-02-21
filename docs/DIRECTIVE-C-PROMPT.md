# Citros — Directive C Design System Prompt

> Paste this prompt before any Composable generation task. It ensures every screen, component, and overlay follows the Citros Directive C visual language exactly.

---

## Role

You are implementing the UI layer of Citros, an AI phone agent for Android (Pixel 10 Pro). Every `@Composable` you write must follow the Directive C design system described below. Do not deviate. Do not invent new tokens. Do not use Material You theming, dynamic color, or `MaterialTheme`. Citros owns its own design language.

---

## 1. Design Philosophy

**iOS-Minimal + Orb Presence.** Flat, opaque, stepped surfaces. No blur, no glassmorphism, no Material ripples. Color is rationed to exactly three touch-points (the Orb, user message bubbles, and the send button). Two additions give the brand mark presence without cost:

1. **Orb Glow** — a `Modifier.shadow()` halo on every Orb instance.
2. **Empty-State Wash** — a `Brush.radialGradient()` at 3% flavor opacity behind the Orb on hero/empty/welcome/done screens only. Disappears the instant content appears.

Everything else is neutral. If a surface or text element is not one of the three touch-points, it uses the stepped surface palette and label hierarchy below.

---

## 2. Spatial Grid

All spacing and sizing derives from a **4dp base unit**.

```kotlin
fun g(n: Float): Dp = (n * 4).dp
fun g(n: Int): Dp = (n * 4).dp
```

Use `g()` for every padding, margin, gap, size, and radius value. Common increments: `g(1)` = 4dp, `g(2)` = 8dp, `g(3)` = 12dp, `g(4)` = 16dp, `g(6)` = 24dp, `g(8)` = 32dp.

**Touch targets:** minimum 44dp (`g(11)`) per Apple HIG, applied universally.

---

## 3. Surface Hierarchy (Opaque, Stepped)

Every surface is opaque. No alpha-blended backgrounds on containers (overlays are the sole exception — they use 0.92 alpha with backdrop blur).

### Dark Theme
| Token         | Hex       | Usage                              |
|---------------|-----------|------------------------------------|
| `bg`          | `#000000` | Screen background                  |
| `surface1`    | `#1C1C1E` | Cards, grouped list sections, inputs |
| `surface2`    | `#2C2C2E` | Assistant bubbles, input fields, nested containers |
| `surface3`    | `#3A3A3C` | Inactive toggles, disabled buttons, tertiary containers |
| `surface4`    | `#48484A` | No-flavor user bubbles             |

### Light Theme
| Token         | Hex       | Usage                              |
|---------------|-----------|------------------------------------|
| `bg`          | `#FFFFFF` | Screen background                  |
| `surface1`    | `#F2F2F7` | Cards, grouped list sections, inputs |
| `surface2`    | `#E5E5EA` | Assistant bubbles, input fields    |
| `surface3`    | `#D1D1D6` | Inactive toggles, disabled buttons |
| `surface4`    | `#C7C7CC` | No-flavor user bubbles             |

---

## 4. Label Hierarchy

### Dark
| Token              | Value                        | Usage                    |
|--------------------|------------------------------|--------------------------|
| `labelPrimary`     | `#FFFFFF`                    | Titles, body text        |
| `labelSecondary`   | `rgba(235,235,245,0.60)`    | Subtitles, descriptions  |
| `labelTertiary`    | `rgba(235,235,245,0.30)`    | Placeholders, captions, disabled |
| `labelQuaternary`  | `rgba(235,235,245,0.18)`    | Inactive icons           |

### Light
| Token              | Value                        | Usage                    |
|--------------------|------------------------------|--------------------------|
| `labelPrimary`     | `#000000`                    | Titles, body text        |
| `labelSecondary`   | `rgba(60,60,67,0.60)`       | Subtitles, descriptions  |
| `labelTertiary`    | `rgba(60,60,67,0.30)`       | Placeholders, captions   |
| `labelQuaternary`  | `rgba(60,60,67,0.18)`       | Inactive icons           |

---

## 5. Separator Tokens

| Token            | Dark                       | Light                    |
|------------------|----------------------------|--------------------------|
| `separator`      | `rgba(84,84,88,0.36)`     | `rgba(60,60,67,0.12)`   |
| `separatorLight` | `rgba(84,84,88,0.20)`     | `rgba(60,60,67,0.06)`   |

Separators are always 0.5dp (`Dp.Hairline` or `0.5.dp`).

---

## 6. System Semantic Colors

| Token    | Dark       | Light      |
|----------|------------|------------|
| `green`  | `#30D158`  | `#34C759`  |
| `red`    | `#FF453A`  | `#FF3B30`  |
| `orange` | `#FF9F0A`  | `#FF9500`  |
| `blue`   | `#0A84FF`  | `#007AFF`  |

---

## 7. Flavor System

Citros has 6 flavors. Flavor color appears in **exactly three places**: the Orb, user message bubbles, and the send button. Nowhere else.

### Flavor Palette

| Key              | `primary`  | `onPrimary` | `glow`                        | `wash`                        |
|------------------|-----------|-------------|-------------------------------|-------------------------------|
| `none`           | —         | —           | dark: `rgba(255,255,255,0.06)` / light: `rgba(0,0,0,0.04)` | `null` (no wash) |
| `lemon`          | `#FFD600` | `#1C1A00`   | `rgba(255,214,0,0.15)`        | `rgba(255,214,0,0.03)`        |
| `tangerine`      | `#FF8C00` | `#FFFFFF`   | `rgba(255,140,0,0.15)`        | `rgba(255,140,0,0.03)`        |
| `lime`           | `#7CB342` | `#FFFFFF`   | `rgba(124,179,66,0.15)`       | `rgba(124,179,66,0.03)`       |
| `blood_orange`   | `#D84315` | `#FFFFFF`   | `rgba(216,67,21,0.15)`        | `rgba(216,67,21,0.03)`        |
| `grapefruit`     | `#E91E63` | `#FFFFFF`   | `rgba(233,30,99,0.15)`        | `rgba(233,30,99,0.03)`        |

### Glow formula — `0.15` alpha
```
rgba(flavor.primary, 0.15)
```

### Wash formula — `0.03` alpha
```
rgba(flavor.primary, 0.03)
```

### Resolution logic

When flavor is `none`:
- Orb color: `labelPrimary` inverted (white on dark, black on light)
- Orb inner: dark `rgba(0,0,0,0.12)` / light `rgba(255,255,255,0.20)`
- Orb glow: `noFlavorGlow` (very subtle neutral — `rgba(255,255,255,0.06)` dark / `rgba(0,0,0,0.04)` light)
- User bubble bg: `surface4` (`#48484A` dark / `#C7C7CC` light)
- User bubble text: `labelPrimary`
- Send bg: `surface3`
- Send icon: `labelSecondary`
- Empty wash: `null` — no wash rendered
- Caret color: `labelSecondary`

When flavor is set:
- Orb color: `flavor.primary`
- Orb inner: `rgba(0,0,0,0.12)` always
- Orb glow: `flavor.glow`
- User bubble bg: `flavor.primary`
- User bubble text: `flavor.onPrimary`
- Send bg: `flavor.primary`
- Send icon: `flavor.onPrimary`
- Empty wash: `flavor.wash`
- Caret color: `flavor.primary`

### Accent color (interactive elements, back buttons, links)
- `none` → `labelSecondary`
- Any flavor → `flavor.primary`

---

## 8. The Orb

The Orb is the Citros brand mark. It appears in the nav bar, overlays, onboarding, settings profile, and empty states at various sizes.

### Structure
A circle with a concentric inner circle at 38% of the outer diameter.

```kotlin
@Composable
fun CitrosOrb(
    color: Color,
    innerColor: Color,
    glowColor: Color?,
    size: Dp,
) {
    val innerSize = size * 0.38f
    Box(
        modifier = Modifier
            .size(size)
            .clip(CircleShape)
            .then(
                if (glowColor != null) Modifier.shadow(
                    blurRadius = size * 0.45f,
                    spreadRadius = size * 0.18f,
                    color = glowColor,
                    shape = CircleShape,
                ) else Modifier
            )
            .background(color),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(innerSize)
                .clip(CircleShape)
                .background(innerColor)
        )
    }
}
```

### Glow sizing rule
Shadow dimensions scale proportionally with orb size:
- **Blur radius** = `size × 0.45`
- **Spread radius** = `size × 0.18`
- **Color** = the glow token (0.15 alpha for flavors, neutral for none)

The glow transitions with `animateColorAsState` over 200ms ease.

### Common orb sizes
| Context           | Size      |
|-------------------|-----------|
| Nav bar           | `g(8)` = 32dp  |
| Panel header      | `g(7)` = 28dp  |
| Dynamic Island    | `g(7)`–`g(8)` = 28–32dp |
| Bubble overlay    | `g(13)` = 52dp |
| Empty state       | `g(14)` = 56dp |
| Settings profile  | `g(14)` = 56dp |
| Onboarding hero   | `g(20)` = 80dp |

---

## 9. Empty-State Wash

A radial gradient behind the Orb on screens with no content (empty chat, welcome, onboarding done). Creates ambient warmth.

```kotlin
Modifier.background(
    Brush.radialGradient(
        colors = listOf(washColor, Color.Transparent),
        center = Offset(0.5f, 0.4f),  // slightly above center
        radiusX = 0.70f,               // 60–70% horizontal
        radiusY = 0.45f,               // 40–45% vertical
    )
)
```

**Rules:**
- Only on empty/hero/welcome/done screens. Never on screens with content.
- Disappears the instant a conversation starts.
- `none` flavor gets no wash (the modifier is simply omitted).
- Wash opacity is always `0.03` of the flavor primary.
- For settings profile, the wash is directional: `ellipse 50% 80%` anchored to the orb position.

---

## 10. Typography

Use system font stack (`FontFamily.Default` on Android resolves to Roboto, which metrically matches SF Pro). Apply **negative letter-spacing** throughout.

| Role            | Size  | Weight    | Tracking  |
|-----------------|-------|-----------|-----------|
| Large title     | 34sp  | Bold 700  | -0.8sp    |
| Title 1         | 28sp  | Bold 700  | -0.8sp    |
| Title 2         | 24sp  | Bold 700  | -0.6sp    |
| Title 3         | 20sp  | Semibold 600 | -0.4sp |
| Headline        | 17sp  | Semibold 600 | -0.4sp |
| Body            | 16–17sp | Regular 400 | -0.2sp |
| Callout         | 15sp  | Regular 400 | -0.2sp   |
| Subheadline     | 14sp  | Regular 400 | -0.15sp  |
| Footnote        | 13sp  | Regular 400 | -0.1sp   |
| Caption 1       | 12sp  | Regular 400 | -0.1sp   |
| Caption 2       | 11sp  | Regular 400 | 0sp      |
| Section header  | 13sp  | Regular 400 | 0.5sp, UPPERCASE |

Line heights: body 22sp, subheadline 20sp, footnote 18sp, caption 16sp.

---

## 11. Icons

All icons are custom SVG. No Material icons.

| Context      | Size    | Stroke   | Cap & Join        |
|-------------|---------|----------|-------------------|
| Navigation  | 20×20dp | 1.5dp    | Round cap, round join |
| Inline      | 14×14dp | 1.2dp    | Round cap, round join |
| Chevron     | 7×12dp  | 1.5dp    | Round cap, round join |
| Back arrow  | 10×16dp | 2.0dp    | Round cap, round join |
| Onboarding  | 28×28dp | 1.8dp    | Round cap, round join |

Fill: `none` (stroke only). Color: passed as parameter, typically `labelSecondary` or `labelTertiary`.

---

## 12. Message Bubbles

### User bubble
- Background: `flavor.primary` (or `surface4` for none)
- Text color: `flavor.onPrimary` (or `labelPrimary` for none)
- Border radius: `18dp 18dp 4dp 18dp` (tail bottom-right)
- Padding: `g(2.5) × g(3.5)` (10dp × 14dp)
- Max width: 82% of container
- Alignment: end

### Assistant bubble
- Background: `surface2`
- Text color: `labelPrimary`
- Border radius: `18dp 18dp 18dp 4dp` (tail bottom-left)
- Same padding, max-width, alignment: start

### Action indicator (inline)
- No bubble container
- Row: `[icon 14dp] [text footnote labelSecondary] [check icon green]`
- Alignment: start

---

## 13. Input Bar

- Container: `surface2`, radius `g(6)` (24dp), min height `g(10)` (40dp)
- Text: body, `labelPrimary`, caret color = accent
- Placeholder: body, `labelTertiary`
- Mic icon: 18×18dp inside the field when empty, `labelSecondary` at 0.6 opacity
- Send button: `g(9)` circle (36dp), background = `sendBg`, icon = `sendIcon`
  - Inactive (no text): background `surface3`, icon `labelQuaternary`
  - Transition: background 150ms ease

---

## 14. Navigation Bar

- Height: `g(11)` (44dp)
- Padding: `g(3) × g(4)` (12dp × 16dp)
- Bottom separator: 0.5dp `separator`
- Layout: `[Orb g(8)] [g(3) gap] [Title column flex] [Settings icon button g(11)×g(11)]`
- Title: headline weight, `labelPrimary`
- Subtitle: footnote, `labelTertiary`

---

## 15. Grouped List (iOS-Style)

### Section
- Outer margin: horizontal `g(4)`, bottom `g(7)`
- Section header: footnote, `labelSecondary`, UPPERCASE, +0.5sp tracking, left-padded `g(4)`, margin-bottom `g(2)`
- Container: `surface1`, radius `g(3)` (12dp), overflow clipped

### Row
- Min height: `g(11)` (44dp)
- Padding: `g(3) × g(4)` (12dp × 16dp)
- Separator: 0.5dp `separatorLight` (not on last row)
- Label: body, `labelPrimary` (or `red` for destructive)
- Detail: footnote, `labelTertiary`
- Trailing: right-aligned, typically a chevron at 0.3 opacity, badge, toggle, or checkmark

### Toggle
- Size: 51×31dp, radius 16dp
- On: accent color (or `green`), knob white with `0 1px 3px rgba(0,0,0,0.3)` shadow
- Off: `surface3`, knob white
- Knob: 27dp circle, translates 20dp

### Badge
- Padding: `g(0.75) × g(2)`, radius `g(1.5)`
- Background: `color` at 18/255 alpha (`${color}18`)
- Text: caption 1, font weight 500, `color`

---

## 16. Segmented Control (Provider Tabs)

- Container: `surface1`, radius `g(2.5)`, inner padding `g(0.75)`
- Tab: flex 1, padding `g(2)` vertical, radius `g(2)`
- Active tab: `surface2` background, `1px solid separator`, `0 1px 3px rgba(0,0,0,0.12)` shadow, font weight 600, `labelPrimary`
- Inactive tab: transparent, font weight 400, `labelTertiary`
- Transition: 150ms

---

## 17. Text Inputs

- Container: `surface1`, radius `g(3)`, height `g(12)` (48dp), horizontal padding `g(3)`
- Text: body, `labelPrimary`
- Placeholder: body, `labelTertiary`
- Caret color: accent
- For API key fields: use monospace font family
- Visibility toggle: eye icon 20×20dp, `labelTertiary`, right-aligned inside container

---

## 18. Buttons

### Primary CTA
- Full width, padding `g(3.5)` vertical (14dp), radius `g(3)` (12dp)
- Background: `btnBg` (= accent for flavored, `labelPrimary` for none)
- Text: headline weight 600, `btnText` (= bg-inverse for none, `onPrimary` for flavored)
- Disabled: `surface2` background, `labelTertiary` text

### Secondary / Ghost
- Same dimensions, `transparent` background, no border
- Text: callout, `labelTertiary`

### "Back" link
- Row: `[BackArrow 8×14dp] [g(1.5) gap] ["Back" headline, accent color]`
- No background, no border

---

## 19. Overlay Modes

### Search Bar (replaces Pixel's Google bar)
Citros overrides the bottom search bar on the Pixel home screen. Same position, same pill shape, but it's the Citros ambient bar.

- **Position:** Docked at bottom of home screen, above gesture handle, below dock row. Replaces the Google search bar 1:1.
- **Shape:** Full-width pill, horizontal margin `g(5)`, height `g(13)` (52dp), radius `g(7)` (full pill)
- **Background:** Theme-aware frosted surface
  - Dark: `rgba(28,28,30,0.88)`, no border, shadow `0 -2dp 16dp rgba(0,0,0,0.3)`
  - Light: `rgba(242,242,247,0.88)`, border `1dp separator`, shadow `0 -2dp 16dp rgba(0,0,0,0.06)`
  - Both: `backdrop-filter: blur(40dp)`
- **Layout:** `[Orb g(9)] [g(2.5) gap] [center content flex] [mic button g(9)]`
- **Orb:** `g(9)` (36dp), with glow, left-aligned (where the Google G logo sits)
- **Center content by state:**
  - *Idle:* `"Ask Citros anything..."` — callout, `labelTertiary`
  - *Executing:* pulsing dot (6dp, `orbColor`, 0.7 opacity) + italic status text (`labelSecondary`) + Stop button (`red` bg, white text, `g(2.5)` radius)
  - *Completed:* bold status text (`labelPrimary`) + green check circle (`g(5)`, `green` at 20% alpha bg, green checkmark)
  - *Failed:* bold status text (`red`) + red alert circle (`g(5)`, `red` at 18% alpha bg, red "!")
  - *Unread:* placeholder text + red badge pill (min `g(5)`, `red` bg, white count text)
- **Mic button:** `g(9)` circle, transparent, mic icon 16×16dp `labelTertiary`. Only visible in idle/unread states.
- **Transition:** 250ms ease on all properties
- **Tap behavior:** Expands to slide-up panel

### Slide-Up Panel
- Radius: `g(4)` top corners
- Background: `rgba(surface1, 0.92)` with `backdrop-filter: blur(40px)` (in Compose: use a semi-transparent `surface1`)
- Grab handle: `g(9)` × 5dp, radius 3dp, `surface3`, centered
- Header row: Orb `g(7)` + title + "Expand" chip (`surface2`, radius `g(3.5)`)
- Input row: same as main chat input bar, separated by 0.5dp `separator`

### Dynamic Island
- Theme-aware: respects dark/light (NOT hardcoded dark)
- Dark: `rgba(28,28,30,0.92)`, shadow `0 4dp 20dp rgba(0,0,0,0.4)`, no border
- Light: `rgba(242,242,247,0.92)`, shadow `0 4dp 20dp rgba(0,0,0,0.10)`, border `1dp separator`
- Both: `backdrop-filter: blur(40dp)`
- Radius: `g(7)` (28dp, full pill)
- Idle: compact — Orb `g(7)` + "Citros" label, min-width 120dp
- Expanded: Orb `g(8)` + content + action, min-width 240dp
- Failed state: Orb turns `red`, glow becomes `rgba(255,69,58,0.15)`
- Transition: 250ms ease on all properties
- Text: `labelPrimary` for titles, `labelSecondary` for subtitles
- Stop button: `red` background, white text, radius `g(3)`

---

## 20. Onboarding Flow

9 steps: Welcome → Appearance → Conversation Style → Getting to Know → API Key → Permissions → Trust → Plan → Done.

### Progress dots
- Gap: `g(1.5)`, height: `g(1.5)` (6dp)
- Active: width `g(4)` (16dp), `orbColor`, full opacity
- Completed: width `g(1.5)` (6dp), `orbColor`, opacity 0.4
- Remaining: width `g(1.5)`, `surface3`
- Radius: `g(1)` (4dp)
- Transition: 200ms ease on width and opacity

### General step layout
- Flex column, padding `g(10) top × g(6) horizontal × g(6) bottom`
- Icon: 28×28dp centered, colored `orbColor`, margin-bottom `g(5)`
- Title: title 2 (24sp/700/-0.6), `labelPrimary`, centered
- Subtitle: callout (15sp), `labelSecondary`, centered, line-height 22sp, margin-bottom `g(6)`
- Content area: flex
- CTA: primary button at bottom
- Optional "Skip for now": ghost button below CTA, margin-top `g(2)`

### Selection cards (Conversation Style, Trust Level)
- Container: `surface1`, radius `g(3)`, padding `g(3.5) × g(4)`
- Selected: `2dp solid orbColor`
- Unselected: `2dp solid transparent`
- Checkmark: `orbColor`, right-aligned

### Flavor picker (Appearance step)
- Swatches: `g(13)` circles (52dp)
- Selected: `3dp solid labelPrimary` + glow preview (`boxShadow: 0 0 16dp 6dp flavor.glow`)
- Unselected: `3dp solid transparent`
- Label below: caption 1, weight 600 if selected

### Interest chips (Getting to Know step)
- Padding: `g(2) × g(3.5)`, radius `g(5)` (full pill)
- Selected: tinted bg (`accent + "18"`), `1.5dp solid accent`, weight 500, accent text
- Unselected: `surface1`, `1.5dp solid transparent`, weight 400, `labelSecondary`

### Plan cards (Paywall)
- Container: `surface1`, radius `g(3)`, padding `g(3.5) × g(4)`
- Selected: `2dp solid orbColor`
- Featured ("Popular") badge: absolute top-right, `orbColor` bg, `btnText` text, 10sp/700/UPPERCASE, radius `0 0 0 g(2)`
- Price: 22sp/700/-0.6 + period in footnote `labelTertiary`
- Feature list: each row = `[10dp checkmark] [g(2) gap] [subheadline text]`
- Checkmark color: `orbColor` when selected, `labelTertiary` when not

---

## 21. Animations & Transitions

- State transitions only. No infinite animation loops on daily-use screens.
- Default duration: 200ms ease for color/opacity, 250ms for layout changes.
- Orb glow: `animateColorAsState`, 200ms.
- Wash: `animateBrushAsState` or crossfade, 300ms.
- Button enable/disable: background 150ms ease.
- Segmented control: 150ms.
- Dynamic Island expand/collapse: 250ms ease.

---

## 22. Hard Rules — Do Not Break

1. **No Material Design.** No `MaterialTheme`, no `Surface()`, no `TopAppBar()`, no Material icons, no ripple effects. Build every composable from raw `Box`, `Row`, `Column`, `Canvas`.
2. **No dynamic color / Monet.** Color comes exclusively from the flavor palette above.
3. **No blur on daily-use screens.** Only overlays use backdrop blur, and only for the panel/island.
4. **Three touch-points only.** Flavor color on the Orb, user bubbles, and send button. Nowhere else. Accent color (links, back buttons, selection borders) also uses `flavor.primary`, but these are interactive affordances, not decorative.
5. **Every Orb gets a glow.** No exceptions. Even the 28dp nav-bar orb.
6. **Wash only on empty screens.** If there is content (messages, list items, form fields), there is no wash.
7. **`none` flavor is fully supported.** No crashes, no invisible elements. Neutral monochrome throughout.
8. **4dp grid.** Every dimension value must be expressible as `g(n)` for some reasonable `n`.
9. **0.5dp separators.** Never 1dp. Never 0dp.
10. **Negative letter-spacing everywhere.** -0.1 to -0.8sp depending on size. Never positive. Section headers at +0.5sp are the only exception.
11. **All surfaces opaque.** No alpha-blended container backgrounds (overlays excepted).
12. **All overlays theme-aware.** Dark and light variants for every overlay mode including the Dynamic Island.

---

## 23. File-Level Checklist

Before submitting any `@Composable`:

- [ ] Uses `g()` for all spacing/sizing
- [ ] Colors come from theme tokens or flavor palette — no hardcoded hex outside of token definitions
- [ ] Orb has glow applied
- [ ] Empty state has wash (or confirms it's a content screen and wash is absent)
- [ ] `none` flavor tested — no null crashes, no invisible elements
- [ ] Dark and light themes both work
- [ ] Touch targets ≥ 44dp
- [ ] Separators are 0.5dp
- [ ] Letter-spacing is negative (except section headers)
- [ ] No Material components used
- [ ] No `MaterialTheme` references
- [ ] Icons are custom SVG paths, not Material icons
