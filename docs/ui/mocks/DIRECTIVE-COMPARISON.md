# Design Directive Pressure Test

Three competing design philosophies applied to the same Fawx chat screen. Each is a real, actively-shipping approach used by major platforms in 2025–2026.

---

## Current — iOS-Minimal (Opaque Surfaces)

**Source lineage:** Apple HIG for iOS 17–18, iMessage, Apple Notes

| Attribute | Value |
|---|---|
| Surface model | Opaque stepped grays (#000 → #1C1C1E → #2C2C2E → #3A3A3C → #48484A) |
| Flavor reach | Three touch-points only: orb, user bubbles, send button |
| "No flavor" | Yes — fully neutral monochrome mode |
| Grid | 4pt base, 8pt standard increment |
| Typography | SF Pro, tight tracking (-0.2 to -0.8) |
| Depth cue | Color step (surface1 vs surface2 vs surface3) |
| Animation | State transitions only |

**Strengths:**
- Maximum clarity and scanability — every element has a hard boundary
- Flavor feels intentional and restrained; "No Flavor" option respects users who want zero personality
- Familiar to the target audience (iOS-first, taste-forward users)
- Simplest to implement — no blur compositing, no palette generation
- Low GPU overhead; great for battery life on a phone daemon running 24/7

**Weaknesses:**
- Can feel flat or sterile, especially in dark mode where #000 → #1C1C1E is a subtle jump
- The three-touch-point rule means most of the UI is identical across flavors — switching from Lemon to Grapefruit changes very little screen area
- Relies heavily on Apple's system colors, which may feel derivative rather than distinct
- No ambient personality — the app doesn't "feel" like Tangerine or Lime in your peripheral vision

---

## Directive A — Material You / Dynamic Color

**Source lineage:** Android 12–15, Google Messages, Pixel UI, Samsung One UI 6

| Attribute | Value |
|---|---|
| Surface model | Tonal palette — 5+ luminance steps derived from flavor hue |
| Flavor reach | Everywhere: surfaces, containers, bubbles, chips, FAB, nav |
| "No flavor" | No — always a palette (closest equivalent: muted gray palette) |
| Grid | 8dp base |
| Typography | Google Sans / Roboto Flex, looser tracking (+0.1 to +0.25) |
| Depth cue | Elevation + tonal distance (surfaceContainer → surfaceContainerHighest) |
| Animation | Emphasized easing (cubic-bezier 0.2, 0, 0, 1), 300ms transitions |

**Strengths:**
- Flavor switch is dramatic and felt across every surface — genuinely feels like a different app
- Tonal harmony is mathematically guaranteed (HCT color space); nothing clashes
- User bubbles use `primaryContainer` which is always readable against `onPrimaryContainer`
- FAB send button follows established M3 convention — Android users get it instantly
- Most expressive approach: the UI *is* the flavor

**Weaknesses:**
- Maximalist — everything tinted means the flavor has no focal point; the orb loses significance when the entire background matches
- Looser typography metrics feel less precise than SF Pro tight tracking (reads "Android" to iOS-biased users)
- No "off switch" — users who want neutral are stuck with a muted palette
- Tonal surfaces in dark mode can feel murky (e.g., Lime dark surfaces have a swamp-green quality)
- Complex to maintain: each flavor requires 16+ derived tokens, and any new surface needs palette integration
- HCT palette derivation requires runtime computation or a large lookup table

---

## Directive B — Translucent Depth / visionOS Glass

**Source lineage:** visionOS, macOS Sonoma–Sequoia, iOS Control Center, Arc Browser

| Attribute | Value |
|---|---|
| Surface model | Frosted glass (backdrop-filter: blur + saturate) over mesh gradient |
| Flavor reach | Ambient: orb glow, background mesh tint, user bubble tint |
| "No flavor" | Yes — glass with no tint, fully neutral |
| Grid | 4pt base (same as current) |
| Typography | SF Pro, same metrics as current |
| Depth cue | Blur intensity + alpha layering (glass over glass) |
| Animation | Continuous ambient (mesh gradient shift), plus state transitions |

**Strengths:**
- Most visually sophisticated — blur + layering creates a sense of physical depth
- Flavor is atmospheric: switching Tangerine → Lime changes the ambient glow, not just dot colors
- Everything feels premium and "spatial era" — aligns with where Apple's own design language is heading
- "No flavor" works beautifully — neutral glass is already an established look (macOS sidebars, visionOS panels)
- User bubbles get a tinted glass treatment that's subtle but distinct from assistant bubbles

**Weaknesses:**
- `backdrop-filter: blur()` is GPU-expensive — on a Pixel 10 running a Rust daemon, this competes for GPU cycles with inference and always-on overlay compositing
- Frosted glass reads poorly at small sizes — the overlay bubble (44×44) won't have enough background content to make blur meaningful
- Accessibility risk: translucent surfaces reduce contrast ratios; WCAG AA compliance requires careful tuning per flavor per theme
- Text over blur is inherently less legible than text over solid surfaces, especially in bright ambient light
- Implementation complexity is high: Jetpack Compose's `Modifier.blur()` is experimental and doesn't perfectly match iOS's UIVisualEffectView
- The mesh gradient background means the app looks different depending on scroll position — could feel unsettled
- In light mode, frosted glass over a light background can look washed out and lose all depth

---

## Head-to-Head Matrix

| Criteria | iOS-Minimal | Material You | Glass Depth |
|---|---|---|---|
| Flavor impact per-switch | Low (3 elements) | High (entire UI) | Medium (ambient) |
| "No Flavor" support | ✅ Clean | ❌ Always tinted | ✅ Clean |
| GPU cost | Lowest | Low | Highest |
| WCAG contrast safety | Safest | Safe (HCT guarantees) | Riskiest |
| Implementation in Compose | Trivial | Medium (palette gen) | Hard (blur APIs) |
| Distinctiveness | Reads "iOS clone" | Reads "Android native" | Reads "premium/spatial" |
| Legibility in sunlight | Best | Good | Worst |
| Battery impact (always-on overlay) | Minimal | Minimal | Significant |
| Design maintenance burden | Lowest (fixed tokens) | Highest (16+ per flavor) | Medium (glass + tint) |

---

## Recommendation

The current iOS-Minimal approach is the strongest choice for this specific product, for three reasons:

1. **Fawx is a daemon.** It runs 24/7 on a phone. GPU budget matters. The overlay (bubble/panel/dynamic island) needs to composite efficiently over any background app. Opaque surfaces are the cheapest to render. Glass blur is the most expensive — and the overlay is where it matters most.

2. **The target user.** "Discerning users with good taste who prefer iOS design standards" — this audience will read Material You as Android-native and Glass Depth as either "trying too hard" or "macOS not phone." The iOS-Minimal approach is native to their expectations.

3. **Flavor as punctuation, not grammar.** The three-touch-point rule makes flavor a deliberate accent. When you see tangerine on the orb, your bubble, and the send button, it registers as a choice. When the entire UI is tangerine (Material You), it's wallpaper — you stop seeing it. The current approach maximizes the *noticeability* of the flavor per pixel of color used.

The one thing the current approach can steal from each alternative:

- **From Material You:** Consider deriving a *single* tinted surface for the empty-state background (e.g., a 3% flavor wash behind the orb on the "How can I help?" screen). This gives flavor a fourth, very subtle touch-point that makes the empty state feel warmer without flooding the conversation view.

- **From Glass Depth:** Consider adding a subtle glow/shadow behind the orb (`box-shadow: 0 0 20px 8px rgba(flavor, 0.15)`). This costs almost nothing to render but gives the orb a sense of presence that pure solid-color orbs lack. The orb is the brand mark — it should feel slightly luminous.
