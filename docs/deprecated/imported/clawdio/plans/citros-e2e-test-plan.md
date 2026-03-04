# Fawx MVP — End-to-End Test Plan

## Test Environment
- **Device:** Pixel 10 Pro (blazer), Android 16
- **ADB:** 100.89.174.76:5555
- **APK:** chat-debug from `feat/android-mvp` (built 2026-02-13)
- **Method:** ADB screenshots + tap/type/swipe automation

---

## 1. App Launch & Fresh State
- [ ] **T1.1** Force-stop and clear data, relaunch → onboarding appears
- [ ] **T1.2** App icon and splash render correctly

## 2. Onboarding Flow
- [ ] **T2.1** Flavor selection screen shows all flavors (Lemon, Tangerine, Lime, Blood Orange, Grapefruit)
- [ ] **T2.2** Selecting a flavor highlights it and enables Continue
- [ ] **T2.3** Personality step renders (name, emoji, vibe inputs)
- [ ] **T2.4** Chat step renders with scripted intro messages
- [ ] **T2.5** Skip button works on chat step
- [ ] **T2.6** API key step renders with provider tabs
- [ ] **T2.7** Entering a valid API key + Test Connection succeeds
- [ ] **T2.8** Entering an invalid API key shows error
- [ ] **T2.9** "Start Chatting" completes onboarding → chat screen
- [ ] **T2.10** Permissions step shows Accessibility + Overlay toggles
- [ ] **T2.11** Ready screen renders with "Start Chatting"

## 3. Chat — Basic Messaging
- [ ] **T3.1** Empty chat shows placeholder/welcome
- [ ] **T3.2** Type a message → send button enables
- [ ] **T3.3** Send a simple message ("Hello") → message appears in chat
- [ ] **T3.4** AI response streams in (loading indicator → response text)
- [ ] **T3.5** Long message scrolls properly
- [ ] **T3.6** Multiple messages maintain conversation context
- [ ] **T3.7** Whitespace-only message doesn't send

## 4. Chat — Phone Control (Agentic)
- [ ] **T4.1** Send "open Settings" → agent reads screen + performs actions
- [ ] **T4.2** Tool results display with 🤖 prefix
- [ ] **T4.3** Screenshot + vision description appears
- [ ] **T4.4** Multi-step task completes (e.g., "open Chrome and go to google.com")
- [ ] **T4.5** Stop button halts execution mid-task
- [ ] **T4.6** Conversational message (≤3 words, no action hint) doesn't trigger tool loop
- [ ] **T4.7** Clipboard read/write works via agent

## 5. Settings / Wallet
- [ ] **T5.1** Gear icon opens Settings screen
- [ ] **T5.2** Settings header shows "Settings" with back button
- [ ] **T5.3** API keys section lists current key(s)
- [ ] **T5.4** Key card shows provider glyph, label, masked key, health dot
- [ ] **T5.5** FAB (+) opens Add Key bottom sheet
- [ ] **T5.6** Add Key: provider chips work (Anthropic/OpenAI/OpenRouter)
- [ ] **T5.7** Add Key: auto-detects provider from key prefix
- [ ] **T5.8** Add Key: Test Connection works
- [ ] **T5.9** Add Key: Save adds key to list
- [ ] **T5.10** Tap key card → sets as active (border highlight)
- [ ] **T5.11** Delete key via button or swipe → confirmation dialog
- [ ] **T5.12** Model selection dropdowns appear for active provider
- [ ] **T5.13** Chat model dropdown shows correct models for provider
- [ ] **T5.14** Action model dropdown shows correct models for provider
- [ ] **T5.15** Back button returns to chat
- [ ] **T5.16** Empty wallet state shows "Add your first key" message

## 6. Overlay (if Accessibility + Overlay enabled)
- [ ] **T6.1** Overlay activates during phone control task
- [ ] **T6.2** Bubble mode renders floating bubble
- [ ] **T6.3** Mini-chat shows current step + progress
- [ ] **T6.4** Send button in mini-chat works
- [ ] **T6.5** Full-app mode shows message input + send
- [ ] **T6.6** Return button in full-app works
- [ ] **T6.7** Stop button halts task from overlay

## 7. Conversation Management
- [ ] **T7.1** Trash icon clears conversation / starts new chat
- [ ] **T7.2** New conversation starts fresh (no prior context)
- [ ] **T7.3** Model indicator shows current model name

## 8. Edge Cases & Error Handling
- [ ] **T8.1** No network → appropriate error message
- [ ] **T8.2** Invalid/expired API key → error shown, not crash
- [ ] **T8.3** Very long message input handles gracefully
- [ ] **T8.4** Rapid send button taps don't duplicate messages
- [ ] **T8.5** Back button from chat doesn't crash
- [ ] **T8.6** Rotate device (if supported) doesn't lose state
- [ ] **T8.7** App backgrounded + foregrounded preserves state
- [ ] **T8.8** Kill and relaunch preserves API key config

## 9. UI Polish / Visual
- [ ] **T9.1** Dark theme renders correctly
- [ ] **T9.2** Status bar + navigation bar colors appropriate
- [ ] **T9.3** Font sizes readable
- [ ] **T9.4** Touch targets adequately sized (48dp minimum)
- [ ] **T9.5** No overlapping/clipped text

---

## Execution Order
1. Fresh install flow (T1 → T2) — requires data clear
2. Settings/Wallet (T5) — test key management
3. Chat basics (T3) — test messaging
4. Phone control (T4) — test agentic features
5. Conversation management (T7)
6. Edge cases (T8)
7. Visual polish (T9)
8. Overlay (T6) — requires permissions, test last
