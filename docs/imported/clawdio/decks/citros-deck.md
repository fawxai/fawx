# Fawx — Pitch Deck
### For: Chris Saum, Active Capital
### Date: February 2026

---

## SLIDE 1: Title

**Fawx**
*AI that uses your phone.*

[Fawx logo / animated sphere visual from fawx.ai]

---

## SLIDE 2: The Problem

**Your phone is the most powerful computer you own. You still operate it like it's 2007.**

- 80+ taps to book a flight
- 15 steps to send an email with an attachment
- You are the bottleneck between intent and action

Every app is a silo. Every task requires you to be the glue between them.

> *Speaker note: This is the Chris Saum tweet energy — "open my email," "pay that invoice," "pull my P&L." People want to say what they want and have it happen.*

---

## SLIDE 3: The Solution

**Fawx is a phone agent. You talk. It does.**

"Send an email to Sarah about tomorrow's meeting"
→ Opens Gmail → Composes → Types → Sends

"Read my Telegram messages"
→ Opens Telegram → Reads conversations → Reports back

No app integrations. No APIs to connect. It operates your phone the way you do — by seeing the screen and touching it.

---

## SLIDE 4: How It Works

**One APK. One setup. Full phone control.**

```
Voice → On-Device STT → AI Reasoning → Phone Actions → Done
         (sherpa-onnx)    (Claude/GPT)   (Accessibility API)
```

- **On-device speech recognition** — no cloud STT dependency, works offline
- **AI reasoning** — understands intent, plans multi-step actions
- **Accessibility Service** — taps, types, swipes, reads screens, takes screenshots
- **Works with every app** — no per-app integrations needed

> *Speaker note: This is the key architectural insight. Instead of building integrations with every app (the way Siri/Google Assistant work), Fawx operates at the OS level. It can use ANY app because it interacts the same way a human does. New app? Already works.*

---

## SLIDE 5: Demo

**[LIVE DEMO or VIDEO]**

Three scenarios, each on a real Pixel phone:

1. **"Send an email to John about the Q4 report"**
   → Opens Gmail → Compose → Fills To/Subject/Body → Sends (10 steps, ~30 seconds)

2. **"Read my latest Telegram messages"**
   → Opens Telegram → Reads → Summarizes (3 steps, ~10 seconds)

3. **"Open my calendar and schedule a meeting for tomorrow at 2pm"**
   → Opens Calendar → Creates event → Sets time → Saves

> *Speaker note: The demo IS the pitch. Let the phone sit on the table, talk to it, watch it work. This is the slide that sells. Record a backup video in case of live demo issues.*

---

## SLIDE 6: Why Now

Four things that didn't exist 18 months ago:

1. **LLMs that can reason about multi-step tasks** — Claude, GPT-4 class models can plan and execute 10+ step phone operations reliably
2. **On-device speech models that actually work** — sherpa-onnx + Parakeet gives us real-time STT without a cloud roundtrip
3. **Cost collapse** — Haiku-class models for action loops, Opus for reasoning. A full phone task costs pennies
4. **Cultural readiness** — People talk to AI now. The behavior is normalized. The gap is: why can't it DO things?

---

## SLIDE 7: Architecture & Moat

**Zero infrastructure. The phone IS the stack.**

| Layer | Technology | Where it runs |
|-------|-----------|---------------|
| Voice Input | sherpa-onnx (Parakeet) | On-device |
| AI Reasoning | Claude / GPT / OpenRouter | Cloud API |
| Phone Control | Android Accessibility Service | On-device |
| Screen Reading | Accessibility tree + screenshots | On-device |
| Voice Output | Piper TTS (coming) | On-device |

**Moat:**
- **No server costs per user** — phone does the compute. We don't run inference.
- **Works with every app, forever** — Accessibility API is an Android platform guarantee
- **On-device voice** — privacy-preserving, low latency, works offline for STT
- **App-agnostic** — competitors build one integration at a time. We got them all on day one.

---

## SLIDE 8: Market

**3.9 billion Android devices worldwide.**

- Voice AI agents market: **$4.7B** today → **$47.5B by 2034** (34.8% CAGR)
- Voice assistant market: **$7.4B** → **$33.7B by 2030**
- Every Android user is a potential Fawx user

**Wedge:** Power users and accessibility-first users who need hands-free phone operation.

**Expand to:** Everyone who's ever wished they could just tell their phone what to do and have it actually work (unlike Siri/Google Assistant).

---

## SLIDE 9: Business Model

**Tiered subscription:**

| Tier | Price | What you get |
|------|-------|-------------|
| **BYO** (Bring Your Own Key) | Free | Full app, use your own AI API keys |
| **Fawx Base** | $9.99/mo | Included AI usage, all models, usage-capped |
| **Fawx Super** | $24.99/mo | Higher usage limits, priority, premium models |

- **BYO tier drives adoption** — zero friction, power users onboard themselves
- **Managed tiers capture value** — most users don't want to manage API keys
- **Usage-capped, NOT model-restricted** — all tiers get all models (Joe's key insight: don't gate features, gate volume)

> *Speaker note: The BYO tier is the growth hack. Enthusiasts install it, show friends, friends want the easy version. The monetization is making AI usage invisible — you just pay Fawx, not Anthropic.*

---

## SLIDE 10: Competitive Landscape

| | Fawx | Google Assistant | Siri | Rabbit R1 / Humane Pin |
|---|---|---|---|---|
| Controls any app | ✅ | ❌ (only integrated apps) | ❌ (only integrated apps) | ❌ (cloud proxy) |
| On-device voice | ✅ | Partial | Partial | ❌ |
| No new hardware | ✅ | ✅ | ✅ | ❌ (dedicated device) |
| LLM-powered reasoning | ✅ | Limited | Limited | ✅ |
| Works offline (STT) | ✅ | ❌ | ❌ | ❌ |
| Multi-step tasks | ✅ | ❌ | ❌ | Limited |
| No infrastructure | ✅ | ❌ (Google servers) | ❌ (Apple servers) | ❌ (cloud) |

**Google and Apple can't do this.** Their assistants are integration-based — they negotiate deals with every app developer. Fawx bypasses the entire integration model by operating at the OS layer.

Rabbit and Humane tried to sell new hardware. The hardware is already in everyone's pocket.

---

## SLIDE 11: Traction & Status

- **Working MVP** on Pixel 10 Pro — real phone control via voice
- **590+ PRs** merged on the codebase
- **270+ automated tests** passing
- **On-device STT** integrated (sherpa-onnx + Parakeet model)
- **Multi-provider AI** — Claude, GPT, OpenRouter all supported
- **7 structured tools** — tap, type, swipe, press_key, open_app, read_screen, scroll
- **Onboarding flow** — install APK, paste key, go

**Distribution:** Sideload via fawx.ai (Play Store won't allow Accessibility Service automation apps — this is a feature, not a bug. It's a barrier to entry for competitors too.)

---

## SLIDE 12: Team

**Joseph Abbud**
Founder & CEO

- **AI & Automation Program Manager, EchoStar** — CFO liaison to AI Strategy Office, drove $20M/year savings through ML automation
- **Founder, YouGroup** — Built real-time GAN-based face filtering and production ML inference for live video
- **Software Engineer, Broad Institute of MIT & Harvard** — Built FireCloud, a cloud genomic analysis platform processing petabytes of data
- **MBA** (UC Denver) + **PMP** certified + BS Biochemistry (Clark University)
- Recent AI projects: Wally (AI paywall generator), 10+ agent builds (RAG, ReAct, MCP), multi-agent systems with cross-instance memory sync

> *Speaker note: The story arc is strong — MIT biotech engineer → founded an AI video startup → enterprise AI at EchoStar → now building the phone agent. Deep technical chops across ML, infrastructure, and shipping products.*

---

## SLIDE 13: The Ask

**Raising: $1M pre-seed**

**Use of funds:**
- **Hire 1 Android engineer** — accelerate device coverage and polish
- **AI usage subsidies** — fund the managed tiers to drive adoption
- **Distribution** — influencer seeding, developer community, sideload onboarding optimization
- **iOS exploration** — accessibility APIs on iOS are more limited but not impossible

**Milestones this capital gets to:**
- 1,000 active sideload users
- Managed tier launch (Fawx Base/Super)
- TTS integration (full voice-in, voice-out loop)
- 3 key use-case demos polished (email, messaging, calendar)

---

## SLIDE 14: Vision

**The last interface.**

Every phone becomes a personal agent. You don't open apps. You don't tap buttons. You say what you want and it happens.

Fawx isn't an assistant that answers questions. It's an agent that takes action.

*"No start menu, no app icons. Just a blank screen and a text box."*
— Chris Saum, Active Capital

> *Speaker note: Yes, quote his own tweet back to him. He described your product before he knew it existed. That's product-market fit.*

---

# APPENDIX (don't present, have ready for questions)

## Technical Deep Dive
- **Accessibility Service**: Android platform API, used by screen readers. Fawx uses it to read UI trees, perform gestures, capture screenshots. No root required.
- **sherpa-onnx**: C++ inference engine for on-device speech models. Parakeet TDT 0.6B (int8 quantized) for STT. Piper VITS for TTS (in progress).
- **Structured Tool Use**: AI model receives 7 tools (tap, type_text, swipe, press_key, open_app, read_screen, scroll) as structured function calls. Each action is verified before proceeding.
- **Dual-model architecture**: Opus/GPT-4 for reasoning, Haiku for action loop execution. Optimizes cost without sacrificing quality.

## Why Not Play Store?
Google Play's Accessibility Service policy explicitly prohibits "automating user actions." This is actually good:
1. It means Google won't build this (conflicts with their own policy)
2. It's a moat — copycats can't just clone and upload to Play Store
3. Sideload distribution gives us direct customer relationship (no 30% cut)

## iOS Path
iOS has limited accessibility automation (UI testing frameworks, Shortcuts). A full Fawx-equivalent would likely require:
- Jailbreak (small market)
- Apple partnership (long-term play)
- Limited "Shortcuts-based" version (near-term)
Android-first is the right call. 3.9B devices, open ecosystem, full accessibility APIs.
