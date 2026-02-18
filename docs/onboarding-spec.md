# Citros Onboarding & Tier Spec

## Overview

Single onboarding flow for both `:chat` and `:app` modules. User picks a tier, enters credentials (or not), and lands in the chat.

---

## Onboarding Flow

### Screen 1: Welcome
- Citros logo + tagline ("AI that uses your phone")
- "Get Started" button

### Screen 2: Choose Your Plan

Three cards:

| Tier | Name | Backend | Models | Price |
|------|------|---------|--------|-------|
| 🔧 | **Bring Your Own** | User's own key | Any supported | Free (user pays provider) |
| 🍊 | **Citros Base** | OpenRouter (Citros account) | All models (Haiku, Sonnet, Opus, GPT-5, etc.) | $X/mo — usage-capped |
| 🚀 | **Citros Super** | OpenRouter (Citros account) | All models (same catalog, higher caps) | $Y/mo — higher usage cap |

### Screen 3a: BYO Key Setup
- Dropdown: Anthropic / OpenAI / OpenRouter
- **Set up API Key** action opens provider key dashboard in Chrome Custom Tabs:
  - Anthropic: `https://console.anthropic.com/settings/keys`
  - OpenAI: `https://platform.openai.com/api-keys`
  - OpenRouter: `https://openrouter.ai/keys`
- Return to app and paste key in a ready text field
- Auto-detect from prefix (`sk-ant-*`, `sk-*`, `sk-or-*`)
- "Test Connection" button + status indicator (valid / invalid / expired)
- On success → Screen 4

### Screen 3b: Citros Tier Setup (Base or Super)
- "Create Account" or "Sign In" (email + password, or Google OAuth)
- Payment method (Stripe)
- On success → provision OpenRouter API key server-side, inject into app
- **Not implemented yet** — show "Coming Soon" with waitlist email capture
- For now: redirect to BYO with a note

### Screen 4: Permissions
- Enable Accessibility Service (required for phone control)
- Grant overlay permission (for `:app` bubble mode)
- Brief explainer of why each is needed

### Screen 5: Ready
- "Start using Citros" → main chat

---

## Tier Architecture

### BYO (Bring Your Own)
- User provides their own API key
- Key stored in SharedPreferences (encrypted via EncryptedSharedPreferences in production)
- Provider auto-detected from key prefix, or manually selected
- Models: provider defaults from `ModelConfig`

### Citros Base
- Backend: OpenRouter with Citros-owned API key
- **All models available** — same catalog as BYO (Haiku, Sonnet, Opus, GPT-5, Gemini, etc.)
- Differentiated by **usage cap**, not model access
- Default model: Haiku 4.5 (cheapest, user can switch)
- Monthly cap: e.g. $10 worth of OpenRouter credits
- Key provisioned server-side, delivered to app via authenticated API call

### Citros Super
- Backend: OpenRouter with Citros-owned API key
- **All models available** — same catalog
- Higher usage cap: e.g. $50 worth of OpenRouter credits
- Default model: Sonnet 4.5 chat + Haiku actions (user can switch)
- Priority queue / higher concurrency

### Model Selection (Both Tiers)
- User picks chat model + action model from dropdown
- Organized by provider: Anthropic (Haiku/Sonnet/Opus), OpenAI (GPT-4o/5/5.2), Google (Gemini), etc.
- Show cost indicator per model ($/M tokens) so users understand cap impact
- Smart defaults: Base defaults to Haiku (stretches cap), Super defaults to Sonnet

### Server-Side (Future)
- Citros API server (TBD — could be simple Express/FastAPI)
- User accounts + Stripe billing
- OpenRouter key pool management (per-user provisioned keys or single key with user tracking)
- Per-user usage tracking against monthly cap
- Usage dashboard in-app (how much cap remaining)
- Endpoints: `/auth/signup`, `/auth/login`, `/subscription/activate`, `/subscription/key`, `/usage/status`

---

## Key Wallet (BYO Tier)

### Concept
A secure credential wallet where users store multiple API keys from different providers and freely switch between them + their available models.

### Current State
- Single `cloud_token` + `cloud_provider` in SharedPreferences
- Switching providers requires signing out and re-entering a key

### Proposed: Key Wallet

Store N keys, each with provider metadata and model catalog:

```
wallet_keys = [
  { id: "key_1", provider: ANTHROPIC, key: "sk-ant-...", label: "Anthropic Personal", addedAt: ... },
  { id: "key_2", provider: OPENROUTER, key: "sk-or-...", label: "OpenRouter", addedAt: ... },
  { id: "key_3", provider: OPENAI, key: "sk-...", label: "OpenAI Work", addedAt: ... }
]
active_key_id = "key_2"
active_chat_model = "anthropic/claude-sonnet-4.5"
active_action_model = "anthropic/claude-haiku-4.5"
```

### Storage
- EncryptedSharedPreferences (Android Keystore-backed) for key values
- JSON array for wallet metadata
- Keys never leave the device (no server sync for BYO)

### Model Catalog Per Provider

Each provider exposes different models:

| Provider | Chat Models | Action Models |
|----------|------------|---------------|
| Anthropic | Opus 4.6, Opus 4.5, Sonnet 4.5, Haiku 4.5 | Haiku 4.5, Sonnet 4.5 |
| OpenAI | GPT-5.2, GPT-5, GPT-4o, o3, o4-mini | GPT-4o-mini, o4-mini |
| OpenRouter | All of the above + Gemini 3, DeepSeek, Llama, etc. | Same |

When user selects a key, the model picker updates to show only models available for that provider.

### UI: Settings → Key Wallet

**Key List:**
- Each key shows: provider icon, label, masked key (`sk-ant-...3f9a`), status dot
- Swipe to delete, tap to edit label
- "Add Key" button at bottom → provider picker + key paste + optional label
- Auto-detect provider from key prefix

**Active Configuration:**
- Selected key highlighted with radio/checkmark
- Below key list: Chat Model dropdown + Action Model dropdown
- Dropdowns filtered to models available for the selected key's provider
- Defaults: smartest chat model + cheapest action model for that provider

**Quick Switcher (in chat):**
- Toolbar shows current provider icon + model name
- Tap → bottom sheet with key wallet + model pickers
- Switch in 2 taps without leaving the conversation

### Implementation Plan

**Phase 1: Wallet storage + migration** (Issue)
- `KeyWallet` data class with serialization
- EncryptedSharedPreferences for key storage
- Migrate existing `cloud_token` → wallet entry on first launch
- `WalletManager` reads/writes wallet, exposes `activeConfig(): ProviderConfig`

**Phase 2: Wallet UI in settings** (Issue)
- Key list with add/edit/delete
- Model pickers (chat + action) per selected key
- Provider auto-detection on key paste

**Phase 3: Quick switcher in chat** (Issue)
- Toolbar indicator
- Bottom sheet wallet picker
- Instant model switch mid-conversation

---

## Data Model

```kotlin
// A single stored credential
data class WalletKey(
    val id: String,           // UUID
    val provider: Provider,
    val label: String,        // User-editable ("Anthropic Personal", "Work OpenRouter")
    val addedAt: Long         // epoch ms
    // Actual key stored separately in EncryptedSharedPreferences keyed by id
)

// Active configuration
data class WalletState(
    val keys: List<WalletKey>,
    val activeKeyId: String?,
    val chatModelId: String,    // User-selected chat model
    val actionModelId: String   // User-selected action model
)

// Resolve to ProviderConfig
fun WalletState.activeConfig(keyStore: EncryptedKeyStore): ProviderConfig? {
    val walletKey = keys.find { it.id == activeKeyId } ?: return null
    val rawKey = keyStore.get(walletKey.id) ?: return null
    return ProviderConfig(
        provider = walletKey.provider,
        baseUrl = walletKey.provider.defaultBaseUrl(),
        chatModelId = chatModelId,
        actionModelId = actionModelId,
        headers = walletKey.provider.buildHeaders(rawKey)
    )
}
```

---

## Migration Path

1. On first launch with new code:
   - Read existing `cloud_token` + `cloud_provider`
   - Create `WalletKey` entry with auto-detected provider
   - Store key in EncryptedSharedPreferences
   - Set as active key with default models for that provider
   - Keep old prefs as read-only fallback

2. `ChatViewModel` reads from `WalletState.activeConfig()` instead of single token

3. Sign-in flow (BYO) writes to wallet instead of `cloud_token`

4. Citros tier (future) injects a managed key into the wallet with `label: "Citros Base"` / `"Citros Super"`
