# Fawx Mockup Revisions Round 2 — Spec Accuracy Fixes

**Context:** Codex reviewed the updated mockups (post-revision-1) against the approved `swift-app-spec.md` and found 3 issues where the mockups diverge from the spec. These need to be fixed before Codex starts building SwiftUI from the screenshots — developers will implement exactly what they see.

**Reference files (attached):**
1. `docs/design/fawx-mockups.html` — current mockups to update
2. `docs/specs/swift-app-spec.md` — the APPROVED spec (source of truth for all enum values and behavior)

**Use the exact same design system** as before. These are surgical fixes, not redesigns.

---

## Fix 1: Thinking Level Enum Values (Screen 11b) — CORRECTED

**Previous revision was wrong.** We incorrectly removed "Adaptive" and added "Medium" / "Extra High". The actual Fawx `ThinkingBudget` enum (source of truth: `fx-config/src/lib.rs`) is:

```rust
enum ThinkingBudget { Adaptive, High, Low, Off }
```

There are exactly **4 levels**: `off, low, adaptive, high`. No "medium", no "extra high".

**Fix:** Change the segmented control to show exactly:

```
Off | Low | Adaptive | High
```

With **"High"** selected (orange highlight).

Four segments instead of five — this fits more comfortably on the iPhone width.

Also update the **macOS Settings screen (7b)** thinking dropdown to list exactly: `off, low, adaptive, high`.

---

## Fix 2: Rate Limited Error State (Screens 12d)

**Problem:** The rate limit error card says "please wait 30 seconds before sending another message." The approved spec (line 942) defines a generic 429 state:

> "Rate limited by LLM provider." Show error card in chat. Do NOT auto-retry message sends. Show a "Retry" button that the user taps explicitly.

The mockup implies a countdown timer that the backend doesn't currently expose (no `Retry-After` header). A developer building from this screenshot would implement a hardcoded 30-second timer that doesn't exist in the API contract.

**Fix:** Change the error card text to match the spec:

- **Card text:** `⚠ Rate limited by LLM provider.`
- **Button:** `Retry` (user-triggered, no countdown)
- **Input placeholder:** `Rate limited — tap Retry above` (instead of "Rate limited — wait...")
- **No countdown timer, no "30 seconds", no "wait" language** that implies automatic timing

Keep the card styling (warning border, surface background) exactly as-is. Just change the copy.

---

## Fix 3: iOS Status Strip — Explicit Single-Line Constraint (Screens 8, 9)

**Problem:** The iOS status strip is better after revision 1 but could still be ambiguous about whether wrapping is allowed. A SwiftUI developer needs to know this is strictly single-line.

**Fix:** Add a small annotation below the status strip on **Screen 8** (the one that already has the model truncation annotation):

> *Status strip is `.lineLimit(1)` — if content overflows, truncate the model name with `…` (e.g., `son…-4-6`). Never wrap to two lines.*

This tells the SwiftUI developer exactly what to do: `lineLimit(1)` with truncation, never height expansion.

If the status strip text currently wraps in the mockup at 390px, also shrink the font or tighten the spacing so it renders on one line in the screenshot itself. The mockup should demonstrate the constraint, not just annotate it.

---

## Summary of Changes

| Screen | What Changes | Type |
|--------|-------------|------|
| 11b (iOS Model Detail) | Segmented control: `Off \| Low \| Adaptive \| High` (4 levels, not 5) | Copy fix |
| 7b (macOS Settings — Model) | Thinking dropdown: `off, low, adaptive, high` | Copy fix |
| 12d (Rate Limited — dark + light) | Card text → "Rate limited by LLM provider." + Retry button, no timer | Copy fix |
| 8 (iOS Sessions) | Add `.lineLimit(1)` annotation to status strip | Annotation |
| 9 (iOS Chat) | Ensure status strip renders on one line | Layout check |

These are all copy/annotation changes — no layout or structural redesign needed.
