# Mock vs Implementation Deviation Report

**Generated:** 2026-02-12  
**Related PR:** #302  
**Status:** Snapshot as of `fix/ui-mock-text-conformance` branch

Comparing `fawx-ui-mocks.html` against the current implementation.

---

## 1. ONBOARDING FLOW — Structural Issues

### 1A. Missing Steps
The mock defines an **8-step onboarding** (tabs: Welcome → Flavor → Personality → Onboard Chat → Paywall → API Key → Permissions → Ready). The code only has **5 steps** (`WELCOME, FLAVOR, PERSONALITY, ONBOARD_CHAT, PAYWALL`).

**Missing screens:**
- **API Key** — In the mock, this is a dedicated onboarding step ("Connect an AI Provider") with provider tabs (Anthropic/OpenAI/OpenRouter), key input, label field, "Test Connection", and "Start Chatting" button. In the code, the API key screen exists **outside** onboarding as a separate `WelcomeConnectScreen` in `ChatActivity.kt` — it shows after onboarding completes, not as part of the flow.
- **Permissions** — Mock has a step showing Accessibility Service + Overlay Permission toggles with "Enable phone control to let Fawx interact with your screen" and "You can enable these later in Settings → Phone Control" skip text. Code has **no permissions onboarding step** — just a purple banner in the main chat.
- **Ready** — Mock has a celebratory "You're all set! Your AI phone agent is ready to go" screen with "Start Chatting" button. Code has **no ready screen** — goes straight from Paywall to the API key screen.

### 1B. Step Counter
- **Mock**: Steps are numbered out of 7 (e.g., "1/7", "3/7")
- **Code**: Steps are numbered out of 4 (e.g., "1/4", "2/4")
- Fix: Update `totalSteps` to match the full flow once missing steps are added

---

## 2. WELCOME SCREEN

| Element | Mock | Code | Deviation |
|---------|------|------|-----------|
| Title | "Welcome to Fawx" | "Welcome to Fawx" | ✅ Match |
| Subtitle | "AI that uses your phone" | "AI that can use your phone for you. Set up your style, choose your plan, and connect your provider." | ❌ Different — mock is short/punchy, code is wordy |
| Hero element | Citrus orb/badge | CitrusHeroBadge | ✅ Likely match |
| CTA button | "Get Started" | "Get Started" | ✅ Match |
| Background | No additional text | N/A | ✅ |

**Action**: Change subtitle to "AI that uses your phone"

---

## 3. FLAVOR SCREEN (Choose Your Flavor)

| Element | Mock | Code | Deviation |
|---------|------|------|-----------|
| Title | "Choose Your Flavor" | "Choose Your Flavor" | ✅ Match |
| Subtitle | "This sets your personal color theme" | "Pick the accent style for Fawx. You can change it later in Settings." | ❌ Different wording |
| Flavor options | 5 flavors | 5 flavors | ✅ Match |
| Step counter | "1/7" | "1/4" | ❌ Wrong total |
| CTA | "Continue" | "Continue" | ✅ Match |

**Action**: Change subtitle to "This sets your personal color theme", fix step counter

---

## 4. PERSONALITY SCREEN

| Element | Mock | Code | Deviation |
|---------|------|------|-----------|
| Title | "Personalize Fawx" | "Conversation Style" | ❌ Different title |
| Subtitle | "Tell me how you like things" | (none) | ❌ Missing subtitle |
| Question 1 | "How should I talk to you?" | "How should I talk to you?" | ✅ Match |
| Question 2 | "How much should I explain?" | "How much should I explain?" | ✅ Match |
| Question 3 | "What's your comfort level?" | "Comfort level" | ❌ Shortened |
| CTA | "Save & Continue" | "Continue" | ❌ Different button text |
| Step counter | "2/7" | "2/4" | ❌ Wrong total |

**Actions**: 
- Change title to "Personalize Fawx"
- Add subtitle "Tell me how you like things"
- Change Q3 to "What's your comfort level?"
- Change button to "Save & Continue"

---

## 5. ONBOARD CHAT

| Element | Mock | Code | Deviation |
|---------|------|------|-----------|
| Title | "Getting to know each other" | "Getting to know each other" | ✅ Match |
| Skip button | "Skip" (pill style) | "Skip" | ✅ Match (verify styling) |
| Step counter | "3/7" | "3/4" | ❌ Wrong total |
| Scripted behavior | Template variables from user input | Ignores user input | ❌ See #287 |
| Identity Summary | Shows extracted chips at end | Shows summary chips | ⚠️ Verify it appears |

---

## 6. PAYWALL SCREEN

| Element | Mock | Code | Deviation |
|---------|------|------|-----------|
| Title | "Choose Your Plan" | "Choose Your Plan" | ✅ Match |
| Plan 1 | "Bring Your Own Key" — Free | "Bring Your Own Key - Free" | ✅ Approximate match |
| Plan 2 | "Fawx Base" — $9/mo | "Fawx Base - $9/mo" | ✅ Approximate match |
| Plan 3 | "Fawx Super" — $29/mo | "Fawx Super - $29/mo" | ✅ Approximate match |
| Skip | "I'll decide later →" | "I'll decide later" (TextButton) | ⚠️ Missing arrow |
| Coming Soon badge | On Base and Super | On Base and Super | ✅ Match |
| BYO subtitle | "All models, no limits — you pay your provider directly" | "Use your own Anthropic, OpenAI, or OpenRouter key" | ❌ Different |
| BYO details | "Try free for 2 days" | "All models, no app-level limits. 2-day trial starts now." | ❌ Different |
| Base subtitle | "All models included — Haiku, Sonnet, Opus, GPT, Gemini" | "All models included with a monthly usage cap" | ❌ Missing model names |
| Super subtitle | "All models included — same full catalog, higher caps" | "Full catalog with higher monthly usage cap" | ❌ Different |
| Pricing footer | "Cancel anytime · Usage resets monthly · All plans include phone control" | Not in code | ❌ Missing |
| Message estimates | "~500 messages/mo on Base · ~5,000 on Super" | Not in code | ❌ Missing |

---

## 7. API KEY SCREEN (Missing from Onboarding)

**Entirely missing as an onboarding step.** The code has a `WelcomeConnectScreen` in `ChatActivity.kt` that shows **after** onboarding, not as part of the onboarding flow.

### Mock design:
- Title: "Connect an AI Provider"
- Subtitle: "Paste your API key to get started"
- Provider selector tabs (Anthropic / OpenAI / OpenRouter)
- "Get a key from [provider URL]" link
- Token input with "Label (optional)" field
- "Test Connection" button
- "Start Chatting" button

### Code design (post-onboarding):
- Title: "Welcome to Fawx"
- 5 stacked buttons: Sign in with OpenAI, Anthropic Key, OpenAI API Key, OpenRouter API Key, Local LLM
- No label field
- "Test Connection" + "Connect" buttons (after selecting provider)

### Key Deviations:
- **Not part of onboarding flow** — should be step 5/7
- **"Sign in with OpenAI" (device code)** — not in mock at all; mock only has API key entry
- **"Local LLM"** — not in mock
- **Missing label field** for naming the key (e.g., "Personal Anthropic")
- **Different layout**: mock has horizontal provider tabs, code has vertical button list
- **"Start Chatting"** vs "Connect" button text

---

## 8. PERMISSIONS SCREEN (Missing)

**Entirely missing from onboarding.** Mock shows:
- Title: "Phone Control"
- "Enable phone control to let Fawx interact with your screen"
- Accessibility Service toggle with description
- Overlay Permission toggle with description  
- "You can enable these later in Settings → Phone Control"
- "Continue" button

Code: Only has a purple banner in the main chat screen ("Enable phone control / Let Fawx see and control your screen")

---

## 9. READY SCREEN (Missing)

**Entirely missing.** Mock shows:
- Citrus badge/orb
- "You're all set!"
- "Your AI phone agent is ready to go"
- "Start Chatting" primary button

---

## 10. MAIN CHAT SCREEN

| Element | Mock | Code | Deviation |
|---------|------|------|-----------|
| Empty state text | "Hey there! What can I help you with?" | "What can I help you with?" | ❌ Missing "Hey there!" |
| Suggestion chips | Present | Present (4 chips) | ✅ Match |
| Model selector pill | "Sonnet 4.5" in header | Shows full model ID "Sonnet 4 5 20250514" | ❌ Too verbose — should be friendly name |
| Input placeholder | "Message Fawx..." | "Message Fawx..." | ✅ Match |
| Phone control banner | Purple banner with "Enable" | Purple banner with "Enable" | ✅ Match |
| Header layout | Fawx name + model pill + settings gear | "Fawx" + model pill + settings/delete/export icons | ⚠️ Mock may have fewer header icons |

---

## 11. SETTINGS SCREEN

| Section | Mock Subtitle | Code Subtitle | Match? |
|---------|---------------|---------------|--------|
| API Keys | "Manage your provider keys" | "Manage provider keys and active account" | ❌ |
| Models | "Chat & action model selection" | "Configure chat and action model defaults" | ❌ |
| Trust Level | "Permission tier settings" | "Control how often Fawx asks before actions" | ❌ |
| Appearance | "Theme & flavor settings" | "Flavor and theme preferences" | ❌ |
| Sound & Haptics | "Voice, sounds, haptic feedback" | **MISSING ENTIRELY** | ❌ |
| Phone Control | "Accessibility & overlay" | **MISSING ENTIRELY** | ❌ |
| About | "Version, licenses" | "Version and platform details" | ❌ |

Additional:
- Profile card with Citrus badge + active key info: ✅ Present
- Models routes to wallet instead of a dedicated models screen: ⚠️ Wrong destination
- Version footer "Made with 🍊 — v0.1.0": In About screen, verify placement

---

## 12. WALLET SCREEN (API Keys Management)

| Element | Mock | Code | Deviation |
|---------|------|------|-----------|
| Title | "API Keys" with "Manage Keys" subtitle | Present in SettingsScreen.kt | ⚠️ Verify exact wording |
| Key cards | Show provider, label, active badge | Present | ✅ Approximate |
| Add Key button | "+ Add Key" | Present | ✅ |
| Active Key indicator | "Active" badge | Present | ✅ |
| Edit details | "Edit details" link | Unclear if present | ⚠️ Verify |

---

## 13. OVERLAY SCREEN

The overlay feature is in PR now per Joe. Mock defines three states:
- **Bubble**: Small floating circle, "Tap to expand"
- **Mini-Chat**: 40% height overlay with chat, "Queue a follow-up...", "Slide to approve" for actions
- **Full App**: Standard full-screen app

This is pending the current PR and will need separate audit.

---

## Priority Fix List (ordered by user impact)

### P0 — Flow-breaking
1. Add **API Key** step to onboarding flow (step 5)
2. Add **Permissions** step to onboarding flow (step 6) 
3. Add **Ready** screen to onboarding flow (step 7)
4. Fix step counters to 7 total

### P1 — Text/Title mismatches
5. Welcome subtitle: "AI that uses your phone"
6. Flavor subtitle: "This sets your personal color theme"
7. Personality title: "Personalize Fawx" + subtitle "Tell me how you like things"
8. Personality Q3: "What's your comfort level?"
9. Personality CTA: "Save & Continue"
10. Chat empty state: "Hey there! What can I help you with?"
11. Model selector: friendly name ("Sonnet 4.5") not raw ID

### P2 — Missing sections
12. Settings: Add "Sound & Haptics" section
13. Settings: Add "Phone Control" section
14. Paywall footer: "Cancel anytime · Usage resets monthly..."
15. Paywall: message estimates text
16. Paywall: "Try free for 2 days" wording

### P3 — Design polish
17. API Key screen: provider tabs (horizontal) vs button list (vertical)
18. API Key screen: add key label field
19. "I'll decide later →" with arrow
20. Onboarding chat: variable substitution (#287)
