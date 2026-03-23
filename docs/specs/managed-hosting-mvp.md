# Managed Hosting MVP — Spec

**Status:** Draft
**Target:** Ship week of 2026-03-23
**Author:** Clawdio + Joe

---

## 1. Overview

Fawx is open source. The iOS/macOS app is open source. Revenue comes from managed hosting: tiered subscriptions that auto-provision a Fawx server, preconfigured with API access, so users get the full Fawx experience without managing infrastructure.

BYOK (Bring Your Own Key) remains first-class and free. Subscriptions are the convenience tier.

---

## 2. Architecture

```
┌─────────────────────┐
│  Fawx iOS/macOS App │
│  (StoreKit 2 subs)  │
└──────────┬──────────┘
           │ HTTPS
           ▼
┌─────────────────────┐
│  Fawx Cloud API     │  ← Provisioning + account mgmt
│  (control plane)    │  ← Hosted on: [TBD — see §3]
└──────────┬──────────┘
           │ provisions
           ▼
┌─────────────────────┐
│  Fawx Instance      │  ← Per-user Fawx server
│  (Fly Machine)      │  ← Runs fawx serve (headless HTTP)
│  (preconfigured)    │  ← API keys injected as secrets
└─────────────────────┘
```

### Components

**A. Fawx Cloud API (Control Plane)**
- Lightweight service that handles:
  - Subscription verification (StoreKit 2 server-to-server notifications)
  - Instance provisioning (create/destroy Fly Machines)
  - Usage metering and budget enforcement
  - Health monitoring
- Language: Rust (reuse Fawx crates) or lightweight Node/Deno service
- Hosted on: single VPS or Fly app

**B. Per-User Fawx Instances**
- Each subscriber gets a dedicated Fly Machine running `fawx serve`
- Machine is stopped when idle (Fly auto-stop), started on first request (Fly auto-start)
- Secrets injected: API keys for subscribed model providers
- Config generated: budget limits matching subscription tier

**C. App Integration**
- StoreKit 2 subscription management
- On subscription confirmation: app receives instance URL + auth token
- All Fawx API calls route to their managed instance
- Seamless fallback: if subscription lapses, app prompts for BYOK setup

---

## 3. Infrastructure: Fly.io

**Why Fly.io for MVP:**
- Machines API: programmatic create/start/stop/destroy
- Auto-stop on idle (no cost when user isn't active)
- Auto-start on HTTP request (wake-on-request)
- Simple secrets management
- Global regions (user picks closest)
- Predictable pricing: ~$3.50/mo for shared-1x-256mb (stopped when idle = fraction of that)

**Per-user machine spec (starter tier):**
- shared-cpu-1x, 256MB RAM (Fawx headless is lightweight)
- 1GB persistent volume (conversation history, memory)
- Auto-stop after 5 min idle
- Auto-start on incoming request

**Per-user machine spec (pro tier):**
- shared-cpu-2x, 512MB RAM
- 5GB persistent volume
- Priority region selection

### Alternative considered: Multi-tenant
- Better margins but requires session isolation, rate limiting, auth scoping in Fawx engine
- Too much engineering for week 1
- Revisit at scale (>1000 subscribers)

---

## 4. Subscription Tiers

| | Free | Starter | Pro |
|---|---|---|---|
| **Price** | $0 | $9.99/mo | $24.99/mo |
| **Infrastructure** | Self-hosted (BYOK) | Managed Fawx instance | Managed Fawx instance |
| **Models** | Whatever you configure | Claude Sonnet + GPT-4.1 | Claude Opus + Sonnet + GPT-4.1 + o3 |
| **Token budget** | Unlimited (your keys) | [TBD — needs cost modeling] | [TBD — needs cost modeling] |
| **Devices** | Unlimited | 2 devices | 5 devices |
| **Memory/History** | Unlimited | 1GB | 5GB |
| **Support** | Community (GitHub) | Email | Priority |

### Pricing notes
- Apple takes 30% year 1, 15% year 2+ (Small Business Program)
- Net revenue at $9.99: ~$7.00 (year 1), ~$8.50 (year 2+)
- Infrastructure cost per user: ~$2-5/mo (Fly machine + API markup)
- API cost is the variable: need usage caps or pass-through billing
- **Web subscription option** at lower price (no Apple cut) — link from app settings

### Token budget modeling (CRITICAL — needs Joe's input)
- Claude Sonnet 4: ~$3/1M input, $15/1M output
- GPT-4.1: ~$2/1M input, $8/1M output
- Claude Opus 4: ~$15/1M input, $75/1M output
- Typical "light user" session: ~50K input + 10K output tokens = ~$0.30/session (Sonnet)
- Typical "power user": 5-10 sessions/day = $1.50-3.00/day = $45-90/mo (Sonnet)
- **Risk:** Power users on $9.99 plan can easily exceed revenue in API costs
- **Mitigation options:**
  - Hard token cap per billing period (e.g., 500K output tokens/mo for Starter)
  - Soft cap with throttling (reduce to smaller model after cap)
  - Usage-based overage charges
  - Rate limiting (requests per minute)

---

## 5. Provisioning Pipeline

### User subscribes (happy path)
```
1. User taps "Subscribe" in iOS/macOS app
2. StoreKit 2 processes payment
3. App receives Transaction
4. App sends Transaction.jwsRepresentation to Fawx Cloud API
5. Cloud API verifies with Apple (App Store Server API)
6. Cloud API provisions Fly Machine:
   a. fly machines create --app fawx-users --config <generated>
   b. Inject API key secrets
   c. Set budget config matching tier
   d. Wait for machine healthy
7. Cloud API returns to app:
   - instance_url: https://<user-id>.fawx-users.fly.dev
   - auth_token: <generated bearer token>
8. App stores credentials in Keychain
9. App connects to managed instance
```

### User cancels
```
1. StoreKit server notification → Cloud API webhook
2. Cloud API marks instance for cleanup
3. Grace period: 7 days (data export available)
4. After grace: destroy Fly Machine + volume
```

### User upgrades/downgrades
```
1. StoreKit notification → Cloud API
2. Cloud API resizes Fly Machine (or creates new + migrates)
3. Update budget config
4. Update API key access (add/remove providers)
```

---

## 6. Fawx Cloud API — Endpoints

```
POST /v1/subscribe
  Body: { transaction_jws: string, device_id: string }
  Returns: { instance_url: string, auth_token: string }

POST /v1/verify
  Body: { auth_token: string }
  Returns: { status: active|expired|grace, tier: string, usage: {...} }

GET /v1/usage
  Headers: Authorization: Bearer <auth_token>
  Returns: { tokens_used: number, tokens_limit: number, period_end: string }

POST /v1/webhook/appstore
  Apple Server-to-Server notification handler
  Handles: SUBSCRIBED, DID_RENEW, DID_CHANGE_RENEWAL_STATUS, EXPIRED, etc.
```

---

## 7. App Changes Required

### StoreKit 2 Integration
- Product configuration in App Store Connect (2 subscription products)
- `Product.products(for:)` to load products
- `product.purchase()` flow
- `Transaction.updates` listener for renewals/cancellations
- Subscription status UI in settings

### Server Connection
- New `ManagedServerProvider` alongside existing BYOK setup
- Stores instance URL + auth token in Keychain
- All existing Fawx API calls route through instance URL
- Health check on app launch; if instance down, show status

### Setup Wizard Update
- New path: "Subscribe for managed hosting" vs "I have my own API keys"
- Subscription picker UI (Starter vs Pro)
- Post-subscribe: automatic connection (no manual server config)

---

## 8. Week 1 Scope (MVP)

### Must have
- [ ] Fly.io account + `fawx-users` app created
- [ ] Provisioning script (CLI, not full API yet): given Apple receipt → create Fly machine
- [ ] `fawx serve` Docker image published to Fly registry
- [ ] StoreKit 2 products configured in App Store Connect
- [ ] App: subscription purchase flow (StoreKit 2)
- [ ] App: managed server connection (store URL + token, route API calls)
- [ ] Cloud API: `/subscribe` and `/webhook/appstore` endpoints
- [ ] Budget enforcement in provisioned instances
- [ ] Basic usage tracking

### Nice to have (week 2+)
- [ ] Web subscription portal (bypass Apple 30%)
- [ ] Usage dashboard in app
- [ ] Auto-scaling machine size based on load
- [ ] Multi-region selection
- [ ] Stripe billing for web subscribers
- [ ] Admin dashboard for monitoring all instances

### Not in MVP
- Multi-tenant architecture
- Custom domain per user
- Team/org accounts
- SLA guarantees

---

## 9. Security Considerations

- **API keys:** Our keys, stored as Fly secrets, never exposed to user
- **Auth tokens:** Generated per-user, stored in Keychain, rotated on subscription renewal
- **Instance isolation:** Each user gets their own Fly Machine (process-level isolation)
- **Data:** Conversation history on per-user Fly volumes; encrypted at rest (Fly default)
- **Network:** HTTPS only, Fly's built-in TLS termination
- **Abuse:** Rate limiting at Cloud API level; budget caps at instance level

---

## 10. Open Questions (Need Joe's Input)

1. **Token budget per tier?** This is the make-or-break pricing decision.
2. **Which cloud API language?** Rust (reuse Fawx crates) vs Node/Deno (faster to ship)?
3. **Fly.io confirmed?** Or preference for another provider?
4. **App Store Connect access:** Do we have an Apple Developer account with subscription capability?
5. **Company entity:** Delaware incorporation still pending — needed for App Store business account?
6. **Launch scope:** Full auto-provisioning, or semi-automated (manual Fly deploys via script) for first week while we build the automation?
7. **Fawx open-source timing:** Open-source the repos before or after managed hosting launches?
8. **API key procurement:** Bulk API keys from Anthropic/OpenAI, or usage-based accounts?

---

## 11. Revenue Projections (Rough)

| Users | Starter ($10) | Pro ($25) | Gross/mo | Net (after Apple) | Infra cost | Profit/mo |
|-------|--------------|-----------|----------|-------------------|------------|-----------|
| 100   | 70           | 30        | $1,450   | ~$1,015           | ~$400      | ~$615     |
| 500   | 350          | 150       | $7,250   | ~$5,075           | ~$1,800    | ~$3,275   |
| 1000  | 700          | 300       | $14,500  | ~$10,150          | ~$3,500    | ~$6,650   |

*Assumes 70/30 Starter/Pro split, 15% Apple cut (year 2), ~$3.50 avg infra/user. API costs NOT included — highly variable and the biggest risk factor.*

---

*This spec is a living document. Update as decisions are made.*
