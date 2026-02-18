# Key Wallet UI Design Brief

> Feed this to Claude Cowork to generate an interactive Compose UI prototype.

## App Context

Citros is an AI phone agent — users chat with an AI that can physically interact with their Android phone (tap, type, launch apps, read screens). The brand identity is 3D citrus fruit with a warm, premium feel. Think: a luxury tool that feels friendly, not enterprise-y.

## What to Design

Create an interactive artifact showing the full Key Wallet settings flow with realistic sample data (3 keys, different providers). Make it look production-ready, not wireframe-y.

## Canonical Interaction Decisions (must follow)

- **Add Key** uses a modal bottom sheet everywhere (onboarding + settings). Do not use dialogs for key entry.
- **Models source of truth** is Settings → Models. Key Wallet shows current model selections and links to "Edit Models."
- **Quick Switcher** is for in-conversation speed only (active key + quick model chips), not advanced wallet editing.
- **Status semantics** always use icon + label + color (`Valid`, `Checking`, `Invalid`). Never color alone.
- **Delete behavior:** deleting the active key requires confirmation, then auto-selects the next valid key if available; otherwise no active key and empty-state guidance.

---

### Screen 1: Key Wallet (Settings → API Keys)

**Key List View:**
- Cards showing stored API keys, each with:
  - Provider icon/logo (Anthropic = coral shield, OpenAI = green/black circle, OpenRouter = blue diamond)
  - User-editable label ("Personal Anthropic", "Work OpenRouter")
  - Masked key preview (`sk-ant-...3f9a`)
  - Status badge with icon + label + color (`Valid`, `Checking`, `Invalid`)
  - Optional secondary status text (e.g., "Checked 2m ago" or short error hint)
  - The active key has a subtle glow, accent border, or highlight treatment
- Tap a card to select it as active
- Swipe left to delete (with confirmation + undo snackbar)
- "Add Key" FAB or prominent button at bottom

**Empty State:**
- Friendly illustration + "Add your first API key" call to action
- Brief explanation: "API keys let you connect directly to AI providers"

---

### Screen 2: Add Key Flow (modal bottom sheet)

- Provider auto-detected from key prefix as user pastes:
  - `sk-ant-*` → Anthropic
  - `sk-or-*` → OpenRouter  
  - `sk-*` → OpenAI
- Provider chips: Anthropic / OpenAI / OpenRouter (auto-selected but manually overridable)
- API key text field (password masked, with show/hide toggle)
- Input behavior: no autocorrect, no suggestions, and secure text handling
- Optional label field (auto-generates smart default like "Anthropic Key")
- "Test Connection" button → spinner → checkmark animation or error state
- Save button (disabled until key and provider are validly parsed)
- If test fails, show explicit inline error copy and keep a secondary "Save anyway" path

---

### Screen 3: Model Summary (inside wallet, linked to full Models screen)

- Show current **Chat Model** and **Action Model** as read-only chips for the active key
- Include "Edit Models" CTA that navigates to Settings → Models (canonical selection screen)
- If active key is invalid, show inline warning with "Fix Key" action instead of model controls
- Keep tier labels and provider-specific filtering consistent with Settings → Models and Quick Switcher

**Available models per provider:**

| Provider | Chat Models | Action Models |
|----------|------------|---------------|
| Anthropic | Opus 4.6, Opus 4.5, Sonnet 4.5, Haiku 4.5 | Haiku 4.5, Sonnet 4.5 |
| OpenAI | GPT-5.2, GPT-5, GPT-4o, o3, o4-mini | GPT-4o-mini, o4-mini |
| OpenRouter | All of the above + Gemini, DeepSeek, Llama, etc. | Same |

---

### Screen 4: Quick Switcher (main chat toolbar integration)

- Small provider icon + abbreviated model name in the chat toolbar
- Tap opens a compact bottom sheet:
  - Key selector (radio list with provider icons + status badge label)
  - Model quick-switch chips
- Goal: switch provider + model in 2 taps without leaving the conversation
- Should feel lightweight — not a full settings screen

---

## Design System

### Colors
- **Primary accent:** Warm citrus orange `#FF8C00` → amber `#FFB300` gradient
- **Surface:** Dark charcoal `#1A1A1A` (dark mode primary)
- **Cards:** Slightly elevated `#242424` with subtle shadow
- **Provider colors:**
  - Anthropic: `#D97757` (coral/terracotta)
  - OpenAI: `#10A37F` (green)
  - OpenRouter: `#6366F1` (indigo)
- **Status:** Green `#22C55E` (valid), Red `#EF4444` (error), Yellow `#EAB308` (checking)

### Typography & Shape
- Material 3 type scale
- Clean, readable body text; slightly playful display/headline font
- Card corners: 16dp rounded
- Bottom sheets: 28dp top corners
- Buttons: fully rounded (pill shape)

### Motion
- Card selection: subtle scale + border glow animation
- Bottom sheet: smooth spring animation
- Status indicator: subtle pulse animation while in `Checking`
- Delete: swipe + fade with undo snackbar
- Provider auto-detect: chip slides in with gentle bounce

### Principles
- **Dark mode primary** (most users), light mode supported
- **Premium but approachable** — luxury tool, not corporate dashboard
- **Information density:** Show what matters, hide complexity
- **Touch-first:** Generous tap targets, swipe gestures, bottom sheets over dialogs
- **Delight:** Micro-animations, provider color accents, smooth transitions

### Accessibility & UX Guardrails
- Text/icon contrast must meet WCAG AA (4.5:1 normal text, 3:1 large text/icons)
- Never rely on color alone for meaning; pair with iconography and labels
- Minimum interactive target size: 48dp
- Support large font scaling without clipping in cards, chips, and sheets
- Respect reduced-motion preference by disabling non-essential bounce/pulse effects
- Destructive actions need confirmation and recovery (undo where possible)

---

## Technical Context

- **Framework:** Jetpack Compose (Material 3 / Material You with dynamic color)
- **Module:** `:chat` (this is the main APK users interact with)
- **Data model (already implemented):**
  ```kotlin
  data class WalletKey(
      val id: String,        // UUID
      val provider: Provider, // ANTHROPIC, OPENAI, OPENROUTER
      val label: String,     // "Personal Anthropic"
      val addedAt: Long      // epoch ms
  )
  
  data class WalletState(
      val keys: List<WalletKey>,
      val activeKeyId: String?,
      val chatModelId: String,
      val actionModelId: String
  )
  ```
- **Additional UI state needed for this flow (derived from secure store + health checks):**
  ```kotlin
  enum class WalletKeyStatus { VALID, CHECKING, INVALID, UNKNOWN }

  data class WalletKeyUiState(
      val keyId: String,
      val maskedPreview: String,   // e.g. sk-ant-...3f9a
      val status: WalletKeyStatus,
      val statusLabel: String,     // "Valid", "Checking", "Invalid"
      val lastValidatedAt: Long?,
      val validationError: String?
  )
  ```
- **Existing auth UI:** Sign-in screen with provider dropdown + key paste field (will be replaced by this wallet flow)

## Sample Data for Prototype

```
Key 1: Anthropic | "Personal Anthropic" | sk-ant-api03-...f9a4 | Active ✅
        Chat: claude-sonnet-4-5 | Action: claude-haiku-4-5

Key 2: OpenRouter | "OpenRouter Pro" | sk-or-v1-...8b2c | Inactive
        Chat: anthropic/claude-sonnet-4.5 | Action: anthropic/claude-haiku-4.5

Key 3: OpenAI | "Work OpenAI" | sk-proj-...d91e | Invalid ❌ (expired key)
        Chat: gpt-4o | Action: gpt-4o-mini
```

## Acceptance Checklist

- [ ] Add Key is a modal bottom sheet in every entry point (no dialog variant)
- [ ] Key rows show health as icon + label + color (`Valid`, `Checking`, `Invalid`)
- [ ] Status includes meaningful text (checked time and/or short error hint when relevant)
- [ ] API key input disables autocorrect/suggestions and supports show/hide securely
- [ ] Save is disabled until provider + key parse successfully
- [ ] Failed test connection shows explicit inline error and still offers "Save anyway"
- [ ] Model controls in wallet are summary-only with an `Edit Models` CTA to Settings
- [ ] Deleting active key follows defined fallback (next valid key or empty-state guidance)
- [ ] Quick switcher key list includes the same health badge semantics as wallet
- [ ] Touch targets are at least 48dp and content remains usable with large font scaling
- [ ] Color contrast and non-color cues meet the documented accessibility guardrails
