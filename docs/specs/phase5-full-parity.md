# Fawx Native App — Phase 5 Full TUI Parity Specification

**Status:** DRAFT
**Phase:** 5 — Full TUI Parity
**Follow-on Phases:** 5.5 — Fleet + Experiments, 6 — Ship
**Targets:** macOS and iOS Swift app clients backed by the existing Fawx HTTP server
**Primary Goal:** Ship the native app as the default daily-driver Fawx interface by reaching practical parity with the TUI where GUI adds clear value, while deliberately leaving conversation-native features in chat.

---

## Table of Contents

1. [Product Goal](#1-product-goal)
2. [Core Philosophy](#2-core-philosophy)
3. [Scope, Non-Goals, and Phase Boundaries](#3-scope-non-goals-and-phase-boundaries)
4. [Relationship to Earlier Swift App Phases](#4-relationship-to-earlier-swift-app-phases)
5. [Phase 5 Product Decisions](#5-phase-5-product-decisions)
6. [Proposal Gate — Two-Tier Permission System](#6-proposal-gate--two-tier-permission-system)
7. [Persistent Permission System and Presets](#7-persistent-permission-system-and-presets)
8. [OpenAI PKCE OAuth](#8-openai-pkce-oauth)
9. [Remote VPS Pairing](#9-remote-vps-pairing)
10. [Skills Marketplace Install and Remove](#10-skills-marketplace-install-and-remove)
11. [Skill Building by Fawx](#11-skill-building-by-fawx)
12. [Cost Tracking](#12-cost-tracking)
13. [Synthesis / Custom Instructions](#13-synthesis--custom-instructions)
14. [Features Intentionally Left to the Agent](#14-features-intentionally-left-to-the-agent)
15. [Settings and Information Architecture Changes](#15-settings-and-information-architecture-changes)
16. [Realtime Event Model and Client State](#16-realtime-event-model-and-client-state)
17. [Backend API Requirements](#17-backend-api-requirements)
18. [Phase 5.5 — Fleet + Experiments](#18-phase-55--fleet--experiments)
19. [Phase 6 — Ship](#19-phase-6--ship)
20. [Implementation Plan](#20-implementation-plan)
21. [Acceptance Criteria](#21-acceptance-criteria)
22. [Open Questions](#22-open-questions)
23. [Appendix A: UX Copy and Interaction Notes](#appendix-a-ux-copy-and-interaction-notes)
24. [Appendix B: Phase 5 Decision Summary](#appendix-b-phase-5-decision-summary)
25. [Appendix C: Phase 5 API Schemas](#appendix-c-phase-5-api-schemas)

---

## 1. Product Goal

Phase 5 is the moment the Fawx Swift app stops feeling like a companion client and becomes the primary product surface.

Phase 4 established a self-contained Mac install, setup wizard, local-server lifecycle, and pairing basics. Phase 5 finishes the job: the native app must cover the high-value operational workflows that currently make the TUI indispensable, while avoiding the trap of turning every TUI command into a new screen.

The defining requirement for this phase is:

> **The native app should feel like Fawx itself, not like a reduced viewer for Fawx.**

This does **not** mean literal screen-for-command parity.

Instead, it means:
- users can safely operate Fawx from the GUI without falling back to terminal for everyday work
- approvals, configuration, pairing, marketplace installs, and account onboarding are first-class native flows
- power-user and introspection workflows that are already naturally conversational remain conversational
- the app is ready to ship publicly as the recommended interface

Phase 5 also lays down the post-ship roadmap:
- **Phase 5.5:** fleet and experiment controls for power users
- **Phase 6:** release hardening, policy docs, branding, distribution, and update infrastructure

---

## 2. Core Philosophy

### 2.1 The Agent Is the Interface

- **DECIDED:** The agent is the primary interface.
- **DECIDED:** A feature that is naturally handled through conversation does **not** automatically deserve a dedicated GUI surface.
- **DECIDED:** The GUI exists to make direct manipulation better where talking would be slower, riskier, more opaque, or more annoying.

This principle is the main filter for Phase 5.

Examples of features that belong in GUI:
- permission approvals
- proposal review with diffs
- provider login flows
- server pairing
- skill marketplace install/remove
- global settings visibility
- usage summary at a glance

Examples of features that stay conversational:
- “What do you remember about X?”
- “Show my loop budget.”
- “Analyze those signals.”
- “Be more concise from now on.”
- “Build me a skill that tracks my portfolio.”

### 2.2 Parity Means Capability, Not UI Duplication

- **DECIDED:** Phase 5 targets **practical TUI parity**, not one-screen-per-command parity.
- **DECIDED:** If the TUI exposes a capability that the agent can already perform safely through chat, the Swift app may surface it via conversation only.
- **DECIDED:** GUI screens are reserved for workflows where review, confirmation, visibility, or direct selection materially improve the experience.

### 2.3 Safety Must Feel Native

- **DECIDED:** The most important Phase 5 work is the proposal gate and permission system.
- **DECIDED:** Safety and autonomy are not hidden implementation details; they are a visible part of the product.
- **DECIDED:** Everyday approvals should be lightweight and in-flow.
- **DECIDED:** High-risk changes should be interruptive, reviewable, and explicit.

---

## 3. Scope, Non-Goals, and Phase Boundaries

### 3.1 In Scope for Phase 5

- Proposal gate with two-tier approval UX
- Session-scoped inline permission prompts
- Persistent per-tool permission settings in Settings and Skills UI
- Permission presets (Safe, Power User)
- OpenAI ChatGPT subscription PKCE OAuth flow via backend HTTP endpoints
- Remote VPS / remote Mac pairing via existing pairing-code system
- Skills marketplace install and remove flows
- Visibility for custom skills built by Fawx itself
- Cost tracking summary and burn warning
- Synthesis / custom instructions visibility and editing in Settings
- Clear articulation of which features remain agent-only
- Backend endpoint contracts for all of the above
- UI and data-model changes needed to make these features robust in the app

### 3.2 In Scope for Phase 5.5

- Fleet dashboard
- Experiment monitor
- Additional backend endpoint contracts for fleet and experiment management

### 3.3 In Scope for Phase 6

- App icon and branding polish
- EULA / Terms of Service
- Privacy Policy
- TestFlight distribution and release preparation
- README and setup guide
- Sparkle update feed setup
- `fawx.ai` download page readiness

### 3.4 Out of Scope for Phase 5

- OpenRouter onboarding UI
- GUI for unsigned/custom skill import from arbitrary local files
- GUI for WASM signing key management
- Dedicated memory browser UI
- Bonjour discovery
- Multi-window macOS architecture
- Deep developer-token accounting and per-turn cost breakdowns
- Broad TUI debug-screen cloning in the GUI

---

## 4. Relationship to Earlier Swift App Phases

Phase 5 builds directly on the capabilities defined in `swift-app-spec.md` and the local-install architecture defined in `phase4-self-contained-install.md`.

### 4.1 Existing Foundation from Phases 1–3

From the approved Swift app spec, the app already has or is expected to have:
- chat-first UI
- session list and conversation management
- SSE streaming
- read-only skills browser
- connection and auth status views
- model and thinking controls
- settings shell and status bar

### 4.2 Existing Foundation from Phase 4

From the self-contained install spec, the app already has or is expected to gain:
- local-server installation and reuse detection
- LaunchAgent-managed local server lifecycle
- setup wizard and provider onboarding basics
- local pairing via QR for iPhone
- menu bar operational controls
- Sparkle update path
- local/remote connection model

### 4.3 What Phase 5 Adds on Top

Phase 5 assumes those foundations and adds the missing “real product” pieces:
- approval UX for autonomy
- persistent permission controls
- remote pairing by typed code
- marketplace actions instead of read-only browsing
- direct user-facing account login for OpenAI subscription users
- transparency around cost and synthesis
- clear separation between “agent should just do this in chat” and “user should control this directly in GUI”

Implementation note:
- Phase 5 should avoid rewriting the Phase 1–4 architecture. The desired outcome is additive evolution, not a second app design.

---

## 5. Phase 5 Product Decisions

### 5.1 Product Framing

- **DECIDED:** Phase 5 is the “ship the app” feature phase.
- **DECIDED:** The proposal gate is the highest-priority feature in this phase.
- **DECIDED:** Fleet and experiments ship immediately after as **Phase 5.5**, not before the initial public app push.

### 5.2 Approval Model

- **DECIDED:** Fawx uses two approval patterns:
  1. lightweight inline permission prompts for routine tool use
  2. heavy proposal review modals for impactful changes

### 5.3 Permission Scope Model

- **DECIDED:** Session-scoped “Allow Always in This Session” decisions are ephemeral and reset when the session ends.
- **DECIDED:** Persistent permission preferences live in Settings and are stored per tool, with skill-aware presentation.

### 5.4 Provider Login Model

- **DECIDED:** Existing CLI OpenAI OAuth logic should be exposed over HTTP so the Swift app can drive it natively.
- **DECIDED:** The target user-facing copy is “I have a ChatGPT subscription,” not “paste an API key,” when subscription OAuth is supported.

### 5.5 Remote Connection Model

- **DECIDED:** Remote VPS and remote Mac pairing in the GUI uses the **existing typed pairing code** flow.
- **DECIDED:** This is distinct from Phase 4 QR-based local iPhone pairing.
- **DECIDED:** QR is not reused for this flow.

### 5.6 Skill Distribution Model

- **DECIDED:** GUI install/remove supports **signed marketplace skills only**.
- **DECIDED:** Unsigned and arbitrary custom-skill installation remains CLI-only.
- **DECIDED:** Skills created by Fawx itself appear in the app under a “Custom Skills” section once built and signed locally.

### 5.7 Cost Visibility Model

- **DECIDED:** Cost tracking is intentionally summary-level only.
- **DECIDED:** The product shows daily, weekly, and monthly spend plus unusual-burn warnings.
- **DECIDED:** Detailed token-per-turn developer accounting stays outside the GUI for now.

### 5.8 Synthesis Model

- **DECIDED:** Synthesis/custom instructions may be changed both conversationally and directly in Settings.
- **DECIDED:** Settings exists primarily for transparency and direct control, not because chat can’t do it.

---

## 6. Proposal Gate — Two-Tier Permission System

This is the centerpiece of Phase 5.

### 6.1 Product Requirement

Fawx must visibly ask before it acts beyond the user’s configured trust boundary, but the approval UX cannot be so heavy that safe mode becomes annoying.

That leads to two distinct approval classes.

### 6.2 Tier 1 — Lightweight Permissions (Inline Chat)

- **DECIDED:** Routine permission requests appear inline, inside the chat flow.
- **DECIDED:** These are for common tool actions where quick yes/no approval is sufficient.
- **DECIDED:** Inline prompts should feel like a natural continuation of the conversation, not like a separate alert system.

Examples:
- “Can I search the web for X?”
- “Can I read this file?”
- “Can I run this shell command?”

### 6.3 Inline Prompt Interaction Model

Each inline prompt should render as a structured permission card in the message stream with concise context and action buttons.

Required actions:
- **Allow**
- **Deny**
- **Allow Always in This Session**

Rules:
- **DECIDED:** “Allow Always in This Session” applies only for the current session.
- **DECIDED:** Session-scoped approvals are in-memory only and disappear when the session ends.
- **DECIDED:** Inline prompts are the normal safe-mode UX for routine operations.

Implementation notes:
- Shell-command prompts should show the actual command string in a monospace block.
- File-read prompts should show target path(s) or path summary.
- Web prompts should show the query.
- Prompts should include the requesting skill/tool when available.

### 6.4 Tier 2 — Heavy Proposals (Modal / Popover)

- **DECIDED:** Some actions are not appropriate for inline yes/no approval.
- **DECIDED:** Self-modification, code edits, config changes, and other impactful mutations should open a dedicated review surface.

Examples:
- skill-registry changes
- local self-modification proposals
- code diffs
- config before/after changes
- destructive system-level proposals

### 6.5 Heavy Proposal Review Requirements

A heavy proposal review surface must support:
- proposal title and rationale
- risk category
- affected files/resources summary
- structured before/after or diff preview
- approve / reject actions
- optional force-approve path when the backend marks a proposal as force-capable

Implementation notes:
- Code diffs should use line-based rendering with additions/deletions highlighted.
- Config proposals should prefer structured before/after comparison over raw blob diff where possible.
- Self-modification proposals should clearly label that the agent is proposing to change its own behavior or tooling.

### 6.6 Relationship Between Inline Prompts and Heavy Proposals

- **DECIDED:** Inline prompt = “Do you permit this tool action?”
- **DECIDED:** Heavy proposal = “Do you accept this meaningful change?”
- **DECIDED:** They are related but not interchangeable.

This distinction matters because approval fatigue is fatal. Routine permission checks must be quick. Rare but consequential changes must be reviewable.

### 6.7 Presentation Rules

- **DECIDED:** Inline permission cards stay in the chat transcript.
- **DECIDED:** Heavy proposals may be launched from chat but review in a modal, sheet, or dedicated detail panel.
- **DECIDED:** Proposal state should still be reflected in the conversation so the transcript remains comprehensible.

Suggested transcript messages:
- “Permission granted for this session.”
- “Proposal approved.”
- “Proposal rejected.”

### 6.8 State and Expiration

- **DECIDED:** Inline permission prompts expire **5 minutes** after creation if the user does not respond.
- **DECIDED:** Proposal review items do **not** expire automatically; they remain pending until explicitly approved or rejected.

Implementation notes:
- During an active inline permission prompt, the agent's SSE turn is **paused**, not failed. The model should stop generating until the prompt is answered or expires.
- If a permission prompt expires, the backend resolves it as `deny`, emits/records an explanatory assistant follow-up such as “Permission request expired, so I treated it as denied,” and lets the agent continue from that denial outcome.
- Pending inline prompts should survive view switches as long as the underlying session remains active.
- The client should reconcile pending prompts and proposals against the backend on reconnect.
- If a prompt or proposal is resolved elsewhere, the UI must collapse stale action buttons into a resolved state.
- Expired inline prompts should render as a non-interactive resolved card with explicit copy such as **Expired — treated as denied**.
- Proposals should preserve their pending state across app restarts, reconnects, and device switches until a user takes an explicit approve/reject action.

---

## 7. Persistent Permission System and Presets

Phase 5 needs both fast temporary approvals and durable settings.

### 7.1 Two Layers of Permission State

- **DECIDED:** Session layer = ephemeral, in-chat overrides
- **DECIDED:** Settings layer = persistent permission preferences

These layers are complementary:
- session approvals reduce friction in a live conversation
- settings establish default trust posture across sessions

### 7.2 Persistent Permission Model

- **DECIDED:** Persistent permissions are defined per tool, with skill-oriented presentation in the UI.
- **DECIDED:** The Skills screen uses **Option B**: each skill shows a gear/settings affordance that opens per-tool permissions.

Example presentation:

```text
Web Search        [Always Allow ▼]
File Read         [Ask Every Time ▼]
File Write        [Ask Every Time ▼]
Shell Commands    [Always Ask ▼]
Self-Modification [Always Ask ▼]
```

Supported states:
- **Always Allow**
- **Ask Every Time**
- **Always Deny**

Implementation note:
- Backend storage should normalize these values as stable enum strings rather than UI copy.

### 7.3 Skill Presentation vs Tool Authority

- **DECIDED:** The UI groups permissions by skill where that helps comprehension.
- **DECIDED:** The underlying permission authority is still per tool/action category.

Rationale:
- Users reason about trust partly in terms of “what this skill can do.”
- But enforcement must remain tool-oriented and consistent across surfaces.

### 7.4 Global Permissions Overview

- **DECIDED:** Settings includes a global permissions overview across all skills/tools.
- **DECIDED:** This overview is the source of truth for persistent defaults.

The global overview should support:
- search/filter by tool or skill
- current effective setting
- preset indicator
- reset-to-default action

### 7.5 Presets

- **DECIDED:** Permission settings tie into presets.
- **DECIDED:** “Safe” sets everything to **Ask Every Time**.
- **DECIDED:** “Power User” allows common low-risk tools by default while keeping destructive/sensitive tools on ask.

Minimum preset set for Phase 5:
- **Safe**
- **Power User**

Optional future preset:
- **Experimental** (explicitly out of scope unless already naturally falls out of implementation)

### 7.6 Effective Permission Resolution Order

Implementation guidance:

Recommended precedence order:
1. explicit session override (allow/deny for current session)
2. persistent tool permission setting
3. preset default
4. backend/system safe default

- **DECIDED:** Persistent enforcement remains **per tool/action category only**.
- **DECIDED:** Skill-level grouping is a presentation aid, not a second override layer in backend policy storage.

### 7.7 Reset and Explainability

Implementation notes:
- Users should be able to understand why something was auto-approved or auto-denied.
- Inline prompts resolved automatically should still be explainable, e.g. “Allowed by Power User preset” or “Allowed by session override.”
- A small “Why?” affordance is preferable to silent magic.

---

## 8. OpenAI PKCE OAuth

Phase 4 intentionally deferred ChatGPT subscription OAuth. Phase 5 brings it into the app.

### 8.1 Product Requirement

- **DECIDED:** Existing CLI OAuth logic should be exposed through HTTP endpoints so the Swift app can initiate and complete OpenAI login natively.
- **DECIDED:** The desired GUI flow is “I have a ChatGPT subscription” rather than “go use terminal.”

### 8.2 User Flow

Target flow:
1. user opens provider setup
2. taps **ChatGPT / OpenAI**
3. chooses **I have a ChatGPT subscription**
4. app requests an OAuth start payload from the backend
5. app launches **`ASWebAuthenticationSession`** with the authorize URL
6. user logs in and authorizes
7. `ASWebAuthenticationSession` captures the redirect natively using the dedicated callback scheme **`fawx-auth://`**
8. app sends auth code + flow token back to backend
9. backend exchanges tokens, stores access + refresh credentials, and marks provider authenticated
10. UI shows authenticated success state

Important routing note:
- **DECIDED:** OAuth uses **`fawx-auth://`**, while Phase 4 pairing continues to use **`fawx://connect`**. These are separate routes and must not be conflated in app URL handling.

### 8.3 PKCE Architecture

- **DECIDED:** OAuth endpoints should be provider-parameterized for consistency with earlier phases: `GET /v1/auth/{provider}/oauth-start`, `POST /v1/auth/{provider}/oauth-callback`, and `POST /v1/auth/{provider}/refresh`.
- **DECIDED:** The app uses `ASWebAuthenticationSession` as the standard Apple OAuth surface instead of a custom `WKWebView` or external-browser handoff.
- **DECIDED:** The backend should keep the PKCE verifier/state server-side and return a short-lived **flow token** to the app.
- **DECIDED:** `POST /v1/auth/{provider}/oauth-callback` accepts auth code + flow token and completes token exchange.

Implementation notes:
- The server owns token exchange, refresh-token storage, access-token refresh, and final credential persistence.
- The app should not persist provider access tokens or refresh tokens directly.
- State/nonce verification should be bound to the backend-managed flow token.
- If refresh later fails, the backend should surface a re-auth-required state so the client can prompt the user to reconnect the provider.

### 8.4 UX Requirements

- The flow should be presented as native account connection, not a developer-only auth hack.
- Error cases should be clear:
  - login cancelled
  - invalid callback state
  - token exchange failed
  - subscription/account not eligible

### 8.5 Relationship to Existing Auth UI

From the earlier Swift app spec, auth management was read-only in V1. Phase 5 changes that assumption for OpenAI subscription setup.

- **DECIDED:** Auth in the GUI is no longer purely read-only.
- **DECIDED:** At minimum, OpenAI subscription setup must be native.
- **OPEN:** Whether Anthropic subscription setup should also be moved fully into a browser-driven native flow in the same phase, or remain on the existing setup-token mechanism for now.

---

## 9. Remote VPS Pairing

Phase 5 adds a new connection flow: pairing the app to an existing remote Fawx install using the current pairing-code system.

### 9.1 Product Requirement

A user who already has Fawx running on a VPS or another Mac should be able to connect from the Swift app without manually copying bearer tokens around.

### 9.2 Canonical Flow

- **DECIDED:** Remote pairing uses the existing pairing flow:
  1. user runs `fawx pair` on the remote machine
  2. remote machine generates a pairing code
  3. in the Swift app, user selects **Connect to a server**
  4. user enters the remote hostname + pairing code
  5. the app calls **`POST /v1/pair/exchange` against that remote host**, not against localhost or any already-connected server
  6. the remote host exchanges the pairing code for a bearer token/session credential
  7. the app stores resulting remote credentials and connects

Callout:
- The remote hostname is required **before** the exchange request can be made. This flow must work even when the app has no existing server connection at all; the typed hostname is the bootstrap target.

### 9.3 Important Boundary

- **DECIDED:** This is **not** the Phase 4 local iPhone QR pairing flow.
- **DECIDED:** Typed pairing code and QR pairing are separate mechanisms for different situations.

### 9.4 UI Requirements

The GUI flow should include:
- hostname field
- pairing code field
- connection test / exchange action
- success state with server nickname or hostname
- failure messages for expired/invalid code, unreachable host, auth exchange failure

### 9.5 Discovery

- **DECIDED:** Optional Tailscale auto-discovery may be added.
- **DECIDED:** It is helpful but not required for the core Phase 5 remote pairing flow.
- **DECIDED:** Typed hostname + pairing code is sufficient for the must-ship path.

### 9.6 Connection Persistence

Implementation notes:
- The app should treat paired remote servers the same way it treats any other stored server connection: canonicalized URL/host identity plus Keychain-stored bearer token.
- If multi-server support is still limited in the app shell, the remote pairing flow should at least cleanly replace the active connection and preserve reconnect behavior.

---

## 10. Skills Marketplace Install and Remove

Phase 1–3 provided a read-only skills browser. Phase 5 turns it into an actionable marketplace surface.

### 10.1 Product Requirement

- **DECIDED:** Users can browse marketplace skills and install/remove them from the GUI.
- **DECIDED:** GUI install/remove is limited to signed marketplace skills from `fawx.ai` / `api.fawx.ai`.

### 10.2 Trust Model

- **DECIDED:** The skill trust model has three tiers:
  1. **Marketplace-signed** — trusted by default for GUI install/remove flows
  2. **Locally-signed by Fawx on this machine** — trusted as locally produced artifacts and shown under **Custom Skills**
  3. **Unsigned** — CLI-only, not installable from the GUI, and requires explicit trust establishment such as `fawx keys trust` before normal use
- **DECIDED:** Local signing uses the **same WASM signature format** as marketplace skills; the difference is the trust root and provenance, not a separate artifact format.
- **DECIDED:** The GUI only installs marketplace-signed skills from the marketplace catalog.
- **DECIDED:** Unsigned/custom/arbitrary local skills remain CLI-only.

Rationale:
- this keeps the consumer-facing GUI safe and understandable
- it avoids inventing a full power-user package-management UI too early
- it aligns with the broader proposal/permission philosophy

### 10.3 Skills View Evolution

The existing skills browser should expand to support at least these sections:
- **Installed Skills**
- **Marketplace**
- **Custom Skills**

Per-skill actions:
- Install
- Remove
- View permissions
- View signature/source metadata if available

### 10.4 Install Flow

Expected install UX:
1. user browses or searches marketplace
2. taps **Install**
3. app calls backend install endpoint by skill name/identifier
4. app shows installing state
5. installed skill appears in Installed Skills

### 10.5 Remove Flow

Expected remove UX:
- swipe action or explicit Remove button
- confirmation for removal if uninstall has downstream effects
- installed skill disappears or moves back to Marketplace section after success

Downstream effects that should trigger stronger confirmation include:
- an active session currently using that skill or one of its tools
- persistent permission entries that will be removed or orphaned
- scheduled/automated workflows that reference the skill
- locally-authored custom instructions or synthesis text that explicitly mentions the skill by name

### 10.6 Marketplace Search Boundary

- **DECIDED:** `GET /v1/skills/search` is a backend proxy to the marketplace API.
- **DECIDED:** The Swift app does not need to talk to `api.fawx.ai` directly.

Implementation note:
- This keeps auth, rate limiting, caching, and signing validation server-side.

### 10.7 Skill Metadata Expectations

Implementation guidance:
- Search results should ideally include name, short description, tools used, publisher, signature status, and install status.
- The backend should normalize marketplace results into the same broad shape as local installed skill summaries where possible.

---

## 11. Skill Building by Fawx

This feature deliberately leans into the principle that the agent is the interface.

### 11.1 Product Requirement

A user should be able to ask Fawx in chat to build a skill, and the resulting skill should show up in the app as a first-class installed custom skill.

### 11.2 Core Behavior

- **DECIDED:** User requests skill creation conversationally.
- **DECIDED:** Fawx generates the WASM skill.
- **DECIDED:** Fawx auto-signs it with a local key.
- **DECIDED:** If no signing key exists, one is generated automatically on first build.
- **DECIDED:** Locally built skills use the same WASM signature format as marketplace skills but are marked as **locally signed** rather than marketplace-signed.
- **DECIDED:** The resulting skill appears in the Skills UI under **Custom Skills**.
- **DECIDED:** No explicit signing-key-management GUI ships in this phase.

### 11.3 Why This Does Not Need a Dedicated Builder Screen

- **DECIDED:** “Build me a skill” is naturally conversational.
- **DECIDED:** The GUI’s role is to surface the result and allow inspection/removal/permission management, not to replace the agent’s authoring workflow.

### 11.4 UX Implications

The Skills screen should distinguish:
- Marketplace skills
- Custom skills built locally by Fawx

Recommended metadata for custom skills:
- built locally badge
- signed locally badge
- created/updated timestamp
- tool list

### 11.5 Safety Implications

- **OPEN:** Whether skill creation that results in new executable capability should always route through the heavy proposal gate before installation, or whether trusted self-built skills can install automatically under some configurations.

Recommended direction:
- marketplace installs can be one-click because trust comes from signature + known source
- Fawx-built custom skills should likely surface a proposal/review moment before activation, especially in safe mode

---

## 12. Cost Tracking

### 12.1 Product Requirement

Users need lightweight visibility into spend without turning the app into a token accounting dashboard.

### 12.2 Summary Model

- **DECIDED:** Show spend summary for:
  - today
  - this week
  - this month
- **DECIDED:** Show a warning indicator when burn rate is unusually high.
- **DECIDED:** Do not ship detailed token-per-turn breakdowns in this phase.

### 12.3 UI Placement

- **DECIDED:** Cost summary may appear in the status bar or in Settings.
- **DECIDED:** A Settings summary is required even if a compact status-bar indicator also exists.

Implementation note:
- On macOS, a compact spend badge in the status bar is useful.
- On iOS, Settings is the safer default summary location unless the main chrome already has room.

### 12.4 Burn Warning Behavior

Implementation guidance:
- The backend should indicate whether usage is merely informational or warning-worthy.
- The client should not invent its own heuristic if the server can provide one.
- Recommended backend baseline: compare today's spend against a rolling **7-day average of completed days** for the same account/profile.
- Suggested thresholds: `info` at **1.5×** baseline, `warning` at **2.0×**, `critical` at **3.0×** or an operator-defined absolute budget cap.
- The backend response should include the computed `baseline_window_days` and `threshold_multiplier` (or equivalent metadata) so the heuristic is inspectable and stable across clients.

Possible warning copy:
- “Usage is higher than your recent average.”
- “High burn rate today.”

### 12.5 Privacy and Presentation

- The UI should frame this as a user-control/trust feature, not as a guilt mechanic.
- Rounded estimates are acceptable if exactness is hard to guarantee cross-provider.

---

## 13. Synthesis / Custom Instructions

### 13.1 Product Requirement

Synthesis is one of the clearest examples of a feature that belongs both in chat and in Settings.

### 13.2 Dual-Path Model

- **DECIDED:** Users can set synthesis conversationally.
- **DECIDED:** Users can also inspect, edit, and clear the persistent synthesis instruction from Settings.

Examples:
- “From now on, be more concise.”
- Settings → Custom Instructions → edit text directly

### 13.3 Why Settings Matters

- **DECIDED:** The Settings field exists for transparency, auditability, and easy correction.
- **DECIDED:** It is not primarily because synthesis cannot be changed via chat.

This avoids the “invisible system state” problem where the agent is following a persistent instruction the user can no longer see.

### 13.4 Editing Rules

The Settings flow should support:
- load current synthesis
- edit and save
- clear entirely
- show empty state when unset
- enforce a server-defined size limit (recommended initial cap: **4,000 UTF-8 characters**) with visible validation before save

### 13.5 Conflict Semantics

Implementation guidance:
- Chat-driven updates and Settings-driven edits should both converge on the same persisted synthesis value.
- The last confirmed write wins.
- The synthesis resource should include a stable version/ETag (or monotonically increasing revision) so the client can detect stale edits deterministically.
- If synthesis is changed in chat while the Settings view is open, the client should either refresh or warn before overwriting stale text.
- `PUT /v1/synthesis` should support conditional write semantics (for example `If-Match` with the returned ETag/version) so a stale save can fail cleanly with 409 instead of silently overwriting newer text.

---

## 14. Features Intentionally Left to the Agent

Phase 5 explicitly rejects the idea that GUI parity means “give everything a panel.”

### 14.1 No Dedicated GUI Required

The following remain conversation-native:

| Feature | TUI Command | GUI Approach |
|---|---|---|
| Budget/loop details | `/budget`, `/loops` | User asks in chat, agent answers |
| Signal debug | `/signals`, `/debug` | User asks in chat, agent explains |
| Signal analysis | `/analyze` | Agent uses internally; answers if asked |
| Self-improvement | `/improve` | Agent decides when to improve; results surface via proposal gate |
| Journal/memory browse | `journal_search` | User asks conversationally; agent searches and answers |

### 14.2 Product Rationale

- **DECIDED:** If the agent can answer the question better than a static panel, the answer belongs in chat.
- **DECIDED:** The GUI should not become a graveyard of thin wrappers around commands.

### 14.3 UX Implication

The chat experience must remain strong enough that “ask the agent” feels like a real first-class path, not a fallback.

---

## 15. Settings and Information Architecture Changes

Phase 5 significantly expands what Settings must do.

### 15.1 New/Expanded Settings Areas

Recommended top-level organization:
- **General**
- **Connection**
- **Permissions & Safety**
- **Providers**
- **Skills**
- **Usage**
- **Custom Instructions**
- **Advanced / Reset**

Accessibility note:
- All new Phase 5 surfaces inherit the Phase 4 accessibility requirement: VoiceOver support, semantic labels, focus order, and non-color-only status signaling are in scope for permission cards, proposal views, diff previews, marketplace flows, usage summaries, and synthesis editing.

### 15.2 Permissions & Safety

This section should include:
- active preset
- global per-tool matrix
- links into per-skill permission sheets
- explanation of session-scoped overrides
- proposal gate behavior summary
- migration/explainability details when older Phase 4 preset/config state has been mapped into the Phase 5 permission system

### 15.3 Providers

This section should now support:
- OpenAI subscription login
- API key fallback where relevant
- provider status
- remove/re-authenticate actions

### 15.4 Skills

This section should now support:
- installed skills
- marketplace search/install/remove entry points
- custom skills visibility
- per-skill permission settings

### 15.5 Usage

This section should show:
- daily / weekly / monthly spend
- burn warning state
- any explanatory note about what is and isn’t counted

### 15.6 Custom Instructions

This section should expose:
- current synthesis text
- edit/save/clear controls
- short explanation that the agent may also update this conversationally

---

## 16. Realtime Event Model and Client State

Phase 5 introduces new kinds of client-visible realtime objects: inline permission prompts and proposals.

### 16.1 Event Delivery

- **DECIDED:** Permission prompt delivery uses the existing SSE stream model.
- **DECIDED:** The client must be able to render prompt events in-flow during streaming or as asynchronous agent-generated requests.
- **DECIDED:** The SSE stream adds a first-class **`permission_prompt`** event type.
- **DECIDED:** When a permission prompt is emitted during generation, the agent's turn pauses until the user responds or the prompt expires. The stream remains open; lack of text deltas during this interval is not a failure.

### 16.2 New Client State Types

Implementation guidance:

The app likely needs distinct client models for:
- pending inline permission request
- resolved inline permission request
- pending heavy proposal summary
- hydrated proposal detail with diff content

### 16.3 Synchronization Rules

- On reconnect, the client should refresh pending proposals from `GET /v1/proposals`.
- If a session contains an inline prompt that was already answered elsewhere, the prompt card should become read-only with the resolved outcome.
- The client should not assume the current device is the only approver.
- If two devices act on the same proposal or prompt concurrently, the first successful resolution wins and subsequent approve/reject/respond attempts should return **409 Conflict** with the resolved status/body so the losing client can update immediately.
- The same conflict rule applies to expired permission prompts: a late response after expiry should return 409 with the terminal expired/denied outcome.

### 16.4 Notification / Attention Model

Implementation note:
- If a proposal arrives while the user is not on the relevant screen, the app should still surface a visible badge or notification state.
- Heavy proposals should be hard to miss.
- Inline prompts should be visible in the transcript and may also need a subtle badge if the conversation is backgrounded.

---

## 17. Backend API Requirements

Phase 5 needs new endpoints and a small expansion of the event/state model established in earlier phases.

### 17.1 New Phase 5 Endpoints

| Endpoint | Method | Purpose |
|---|---|---|
| `/v1/proposals` | GET | List pending or recent proposals |
| `/v1/proposals/{id}` | GET | Fetch proposal detail including diff preview |
| `/v1/proposals/{id}/approve` | POST | Approve proposal, optionally force |
| `/v1/proposals/{id}/reject` | POST | Reject/archive proposal |
| `/v1/permissions` | GET | Read persistent permission settings |
| `/v1/permissions` | PATCH | Update persistent permission settings |
| `/v1/permissions/prompts/{id}/respond` | POST | Respond to an inline permission prompt |
| `/v1/auth/{provider}/oauth-start` | GET | Start provider PKCE OAuth flow |
| `/v1/auth/{provider}/oauth-callback` | POST | Complete provider OAuth token exchange |
| `/v1/auth/{provider}/refresh` | POST | Refresh provider access token using stored refresh credentials |
| `/v1/skills/search` | GET | Search marketplace skills via backend proxy |
| `/v1/skills/install` | POST | Install signed marketplace skill |
| `/v1/skills/{name}` | DELETE | Remove installed skill |
| `/v1/usage` | GET | Cost summary and burn warning |
| `/v1/synthesis` | GET | Read current synthesis instruction |
| `/v1/synthesis` | PUT | Set/update synthesis instruction |
| `/v1/synthesis` | DELETE | Clear synthesis instruction |
| `/v1/pair/exchange` | POST | Exchange typed pairing code + host for token/session connection |

### 17.2 Existing Systems Reused in Phase 5

Phase 5 intentionally reuses:
- existing SSE infrastructure for live permission prompts
- existing pairing system for remote server onboarding
- existing skills list as the base for installed/custom skill presentation
- existing auth architecture, extended with OpenAI OAuth endpoints

### 17.3 Endpoint Design Rules

#### Proposal endpoints must separate summary vs detail

- **DECIDED:** Proposal list and proposal detail are separate calls.
- List responses should be lightweight.
- Diff blobs belong in detail responses.

#### Permissions endpoint must be patch-friendly

- **DECIDED:** Persistent permission updates are partial overlays, not blind full replacements.
- This preserves compatibility with future tools/settings growth.

#### Usage endpoint should be summary-first

- **DECIDED:** `/v1/usage` is for UI summary data, not developer telemetry dumps.

#### Pair exchange should remain simple

- **DECIDED:** Remote pairing should be completable with hostname + pairing code.
- **DECIDED:** `/v1/pair/exchange` is called against the **remote host supplied by the user**, not against localhost or the app's currently connected backend.
- The app should not need shell access or manual token copy.

#### OAuth refresh should be explicit in the contract

- **DECIDED:** The engine stores refresh tokens server-side and refreshes provider access tokens before expiry.
- **DECIDED:** If refresh fails, the backend should surface a re-auth-needed state and the client should show an inline/native reconnect prompt instead of silently failing future requests.

### 17.4 SSE Additions for Inline Permission Prompts

Implementation guidance:
- Existing stream event handling should be extended with a concrete **`permission_prompt`** SSE event as defined in Appendix C.
- The frontend contract should make prompt identity stable so responses can target a specific pending request.
- The stream should remain open while awaiting a response; the paused interval is part of normal stream lifecycle.
- If multiple prompts are generated close together, the client should queue them in transcript order. A session-scoped approval that covers the same tool category may auto-resolve later queued prompts, but the transcript should still show each prompt and its resolved reason.
- Approval/reject/respond endpoints should be idempotent enough for tap retries and should enforce sensible rate limiting on repeated submissions.

### 17.5 Phase 5.5 Endpoint Families

Fleet and experiment endpoints are specified later in this document because they belong to the immediate post-ship follow-on phase.

---

## 18. Phase 5.5 — Fleet + Experiments

Phase 5.5 ships immediately after the main app release.

### 18.1 Why Phase 5.5 Exists Separately

- **DECIDED:** The app should ship first.
- **DECIDED:** Fleet and experiments are high-value differentiators, but they should not hold the initial app release hostage.

### 18.2 Fleet Dashboard

#### Product Goals

- node health monitoring
- task dispatch across machines
- workload visualization
- add/remove nodes from GUI
- clear “manage your AI fleet from your phone” story

#### Expected UI Areas

- **Fleet Overview**
  - total nodes
  - healthy / degraded / offline counts
  - current workloads
- **Node List**
  - hostname / label
  - status
  - last seen
  - active tasks
  - capability badges
- **Node Detail**
  - current workload
  - recent runs
  - add/remove or pause node actions
- **Dispatch UI**
  - send task to selected node or auto-place

#### Core Decisions

- **DECIDED:** Fleet is a power-user differentiator, not required for initial app ship.
- **OPEN:** Whether node enrollment/removal in GUI should use the same pairing primitives as remote-server app connection, or a distinct fleet enrollment protocol.

### 18.3 Experiment Monitor

#### Product Goals

- run proof-of-fitness experiments
- inspect chains, scores, and tournament results
- present experiments as a paired feature with fleet, since experiments run across fleet nodes

#### Expected UI Areas

- experiment list
- experiment detail
- run status
- scoreboards
- chain/tournament visualization
- result drill-down

#### Core Decisions

- **DECIDED:** Fleet dashboard and experiment monitor ship together as one power-user package.
- **DECIDED:** Experiments are a headline capability, but not part of the initial “ship the app” gate.

### 18.4 Fleet API Requirements

Minimum endpoint family for Phase 5.5:

| Endpoint | Method | Purpose |
|---|---|---|
| `/v1/fleet/nodes` | GET | List nodes and health summary |
| `/v1/fleet/nodes/{id}` | GET | Node detail |
| `/v1/fleet/nodes/{id}` | DELETE | Remove node from fleet |
| `/v1/fleet/nodes/{id}/tasks` | POST | Dispatch task to node |
| `/v1/fleet/overview` | GET | Aggregate fleet metrics |

### 18.5 Experiment API Requirements

Minimum endpoint family for Phase 5.5:

| Endpoint | Method | Purpose |
|---|---|---|
| `/v1/experiments` | GET | List experiments |
| `/v1/experiments` | POST | Create/start experiment |
| `/v1/experiments/{id}` | GET | Experiment detail |
| `/v1/experiments/{id}/results` | GET | Scores, chains, tournament results |
| `/v1/experiments/{id}/stop` | POST | Stop/cancel running experiment |

### 18.6 Data Model Guidance

Implementation notes:
- Fleet endpoints should return stable node IDs, display names, health state, capabilities, and timestamps.
- Experiment endpoints should return both machine-readable status and UI-friendly summary fields.
- Long-running experiment progress should ideally reuse SSE or an event stream instead of pure polling.

---

## 19. Phase 6 — Ship

Phase 6 is the release hardening and packaging pass after the app is functionally ready.

### 19.1 Release Requirements

- **DECIDED:** App icon and branding must be finalized.
- **DECIDED:** EULA and Terms of Service need draft-ready text, with optional lawyer review.
- **DECIDED:** Privacy Policy must exist before broad release.
- **DECIDED:** TestFlight distribution is part of the ship phase.
- **DECIDED:** README and setup guide must exist.
- **DECIDED:** Sparkle auto-update feed must be configured.
- **DECIDED:** `fawx.ai` download page must be ready.

### 19.2 Relationship to Phase 4

Phase 4 already selected Sparkle and the direct-download distribution model. Phase 6 turns that decision into actual public release readiness.

### 19.3 Non-Code Ship Work

This phase includes product assets and trust/legal basics, not just engineering tasks.

Implementation note:
- The legal/policy documents do not need to be solved by the app team alone, but the app cannot be considered shipped publicly without them.

---

## 20. Implementation Plan

### 20.1 Workstreams

1. **Proposal gate backend + event model**
   - proposal list/detail/approve/reject endpoints
   - SSE prompt delivery
   - inline permission response endpoint/event contract

2. **Proposal gate UI**
   - inline permission cards in chat
   - proposal modal/detail viewer
   - resolved-state rendering

3. **Persistent permissions + presets**
   - permission settings storage and API
   - settings UI
   - per-skill permissions sheet
   - preset application and explainability

4. **OpenAI OAuth**
   - PKCE HTTP endpoints
   - in-app browser flow
   - callback handling
   - auth success/failure state integration

5. **Remote VPS pairing**
   - hostname + pairing code onboarding UI
   - token exchange plumbing
   - stored connection update logic

6. **Skills marketplace actions**
   - marketplace search endpoint/UI
   - install/remove actions
   - installed vs marketplace vs custom sections

7. **Usage + synthesis**
   - usage summary endpoint/UI
   - synthesis CRUD endpoint/UI
   - stale-edit conflict handling

8. **Phase 5.5 planning hooks**
   - define fleet/experiment endpoint contracts now
   - avoid app architecture dead ends that would block those screens

9. **Phase 6 release preparation**
   - icon/branding/policy/distribution/update feed tasks after feature completion

### 20.2 Suggested Implementation Order

1. Proposal/permission backend contracts
2. Inline permission UI + heavy proposal viewer
3. Persistent permission settings + presets
4. OpenAI OAuth flow
5. Remote VPS pairing UI
6. Marketplace install/remove actions
7. Usage summary + synthesis settings
8. Fleet/experiment API groundwork
9. Ship polish and release infrastructure

### 20.3 Architectural Cautions

- Avoid inventing a second unrelated approval system in the client.
- Keep persistent permissions patch-based and extensible.
- Do not let marketplace skill UX bleed into arbitrary local package management.
- Ensure chat transcript remains intelligible when approvals/proposals occur.
- Design data models with Phase 5.5 list/detail screens in mind.

---

## 21. Acceptance Criteria

Phase 5 is complete when a user can, from the Swift app alone:
- receive routine permission requests inline in chat and approve/deny them
- review heavy proposals with diffs and approve/reject them
- set persistent tool permissions and switch presets
- log in with ChatGPT subscription OAuth through a native flow
- connect to a remote VPS/Mac using hostname + pairing code
- browse marketplace skills and install/remove signed skills
- see custom skills built by Fawx in the Skills UI
- view usage summaries and burn warnings
- inspect/edit/clear persistent custom instructions
- continue relying on chat for features intentionally left agent-native

Phase 5.5 is complete when a power user can:
- see fleet health in the app
- inspect node details
- dispatch work across nodes
- run and monitor experiments
- inspect scores/tournament outcomes

Phase 6 is complete when:
- branding assets are finalized
- legal/policy docs exist
- TestFlight build is distributable
- direct-download update feed is configured
- `fawx.ai` download page is ready

---

## 22. Open Questions

The major product decisions are settled. Remaining questions are implementation and contract details.

### OPEN 1 — Inline Permission Response Endpoint Shape

The planning decisions require realtime delivery of prompts and some way to answer them, but the exact endpoint/path for responding to a specific inline prompt is still open.

Suggested direction:
- `POST /v1/permissions/prompts/{id}/respond`
- request includes `decision` and optional `scope`

### OPEN 2 — Proposal Category Taxonomy

What exact category enum should proposals use for reliable UI behavior?

Suggested baseline:
- `self_modification`
- `code_change`
- `config_change`
- `skill_install`
- `other`

### OPEN 3 — Fawx-Built Skill Activation Gate

Should locally generated custom skills auto-install after successful build/sign, or must activation always go through a heavy proposal review step?

### OPEN 4 — Tailscale Auto-Discovery Scope

For remote pairing, do we ship only manual hostname entry in Phase 5, or do we include Tailnet discovery if the implementation is straightforward enough?

### OPEN 5 — Anthropic Auth UX Alignment

Should Anthropic remain on the setup-token path while OpenAI gets native PKCE OAuth, or should provider onboarding be further normalized in the same release window?

### OPEN 6 — Fleet Enrollment Protocol

Should Phase 5.5 fleet node addition reuse the existing pairing model directly, or does fleet management need a separate enrollment/auth story?

---

## Appendix A: UX Copy and Interaction Notes

### A.1 Inline Permission Prompt Copy

Recommended concise labels:
- **Allow**
- **Deny**
- **Allow Always in This Session**

Examples:
- “Fawx wants to search the web for: `swift pkce oauth callback capture`”
- “Fawx wants to read: `~/fawx/config.toml`”
- “Fawx wants to run this command:”

### A.2 Heavy Proposal Copy

Recommended headings:
- **Review Proposal**
- **Why Fawx is asking**
- **What will change**
- **Approve** / **Reject**

### A.3 Permission Explainability

Recommended small-print status lines:
- “Allowed by session override”
- “Allowed by Power User preset”
- “Denied by persistent permission setting”

### A.4 Remote Pairing Copy

Recommended form labels:
- **Server Hostname**
- **Pairing Code**
- **Connect to Server**

Recommended helper copy:
> Fawx will contact the remote server you entered here to exchange the pairing code. This does not go through localhost first.

### A.5 Usage Copy

Recommended summary labels:
- **Today**
- **This Week**
- **This Month**
- **High usage today**

If the marketplace is unavailable, recommended fallback copy:
- **Marketplace temporarily unavailable**
- **Showing installed and cached results only**

### A.6 Custom Instructions Copy

Recommended section copy:

> **Custom Instructions** let you set persistent guidance for how Fawx should behave. You can also change this by asking Fawx directly in chat.

---

## Appendix B: Phase 5 Decision Summary

### Core Philosophy
- **DECIDED:** The agent is the interface
- **DECIDED:** GUI surfaces only what direct interaction improves
- **DECIDED:** Parity means capability parity, not command-to-screen duplication

### Proposal Gate
- **DECIDED:** Two approval tiers: inline lightweight permissions + heavy review proposals
- **DECIDED:** Inline prompts are the everyday safe-mode UX
- **DECIDED:** Heavy proposals handle self-modification, code diffs, config changes, and similar high-impact actions
- **DECIDED:** Session-scoped allow-always decisions are ephemeral

### Permissions
- **DECIDED:** Persistent permissions live in Settings
- **DECIDED:** Skills UI presents per-tool permissions with a gear/settings affordance
- **DECIDED:** Supported states are Always Allow / Ask Every Time / Always Deny
- **DECIDED:** Safe preset = Ask Every Time for everything
- **DECIDED:** Power User preset = common tools allowed, destructive tools still ask
- **DECIDED:** Persistent authority remains per-tool; skill grouping is presentational only

### Auth and Pairing
- **DECIDED:** OpenAI ChatGPT subscription login uses PKCE OAuth via backend HTTP endpoints
- **DECIDED:** OAuth is launched with `ASWebAuthenticationSession` and uses the dedicated `fawx-auth://` callback scheme
- **DECIDED:** Backend-managed refresh tokens are part of the OAuth lifecycle
- **DECIDED:** Remote VPS/Mac pairing uses existing typed pairing code flow
- **DECIDED:** `/v1/pair/exchange` is called against the remote host entered by the user
- **DECIDED:** Remote pairing is distinct from Phase 4 QR pairing

### Skills
- **DECIDED:** GUI can install/remove signed marketplace skills
- **DECIDED:** GUI only handles signed marketplace installs
- **DECIDED:** Unsigned/custom installs remain CLI-only
- **DECIDED:** Trust tiers are marketplace-signed, locally signed by Fawx, and unsigned
- **DECIDED:** Fawx-built skills appear under Custom Skills
- **DECIDED:** Signing key generation/signing is transparent with no key-management GUI

### Usage and Synthesis
- **DECIDED:** Usage is summary-level only (day/week/month + warning)
- **DECIDED:** Synthesis can be changed via chat or Settings
- **DECIDED:** Settings exists for transparency and direct editing

### Agent-Native Features
- **DECIDED:** Budget/loop details remain conversational
- **DECIDED:** Signal debug/analysis remain conversational
- **DECIDED:** Self-improvement remains agent-driven with proposal-gate surfacing
- **DECIDED:** Memory/journal browse remains conversational

### Phase 5.5 and Phase 6
- **DECIDED:** Fleet + experiments ship immediately after the app ships, as Phase 5.5
- **DECIDED:** Phase 6 covers icon/branding, policy docs, TestFlight, Sparkle feed, and download-page readiness

---

## Appendix C: Phase 5 API Schemas

All endpoints follow the standard error response format defined in `swift-app-spec.md` Appendix A:

```json
{ "error": "<message>" }
```

Use appropriate HTTP status codes such as 400, 401, 404, 409, 422, 500, and 503.

This appendix defines the request/response shapes for the Phase 5 endpoint surface, plus the Phase 5.5 fleet/experiment contracts that should be stabilized early.

### 1. List Proposals — `GET /v1/proposals`
```json
{
  "proposals": [
    {
      "id": "prop_01HZZXYZ",
      "title": "Update skill registry for new marketplace install",
      "category": "self_modification",
      "summary": "Fawx wants to add the signed skill 'portfolio-tracker' to the local skill registry.",
      "status": "pending",
      "force_allowed": false,
      "created_at": 1741977600,
      "session_key": "sess-a1b2c3d4"
    }
  ],
  "total": 1
}
```
**Notes:**
- `category` suggested values: `self_modification` | `code_change` | `config_change` | `skill_install` | `other`
- `status` suggested values: `pending` | `approved` | `rejected` | `expired`
- List response should stay lightweight; no full diffs here

### 2. Get Proposal Detail — `GET /v1/proposals/{id}`
```json
{
  "id": "prop_01HZZXYZ",
  "title": "Update skill registry for new marketplace install",
  "category": "self_modification",
  "summary": "Fawx wants to add the signed skill 'portfolio-tracker' to the local skill registry.",
  "reason": "The user requested installation of a signed marketplace skill, which requires updating local registry state.",
  "status": "pending",
  "force_allowed": false,
  "session_key": "sess-a1b2c3d4",
  "created_at": 1741977600,
  "diff": {
    "kind": "unified",
    "files": [
      {
        "path": "~/.fawx/skills/registry.json",
        "language": "json",
        "preview": "@@ -1,3 +1,4 @@\n ..."
      }
    ]
  },
  "metadata": {
    "risk_level": "medium",
    "affected_resources": [
      "~/.fawx/skills/registry.json"
    ]
  }
}
```
**Notes:**
- `diff.kind` could later support structured config comparisons in addition to unified diffs
- `risk_level` suggested values: `low` | `medium` | `high`

### 3. Approve Proposal — `POST /v1/proposals/{id}/approve`
Request:
```json
{
  "force": false
}
```
Response:
```json
{
  "id": "prop_01HZZXYZ",
  "approved": true,
  "status": "approved",
  "force": false,
  "resolved_at": 1741977660
}
```
**Notes:**
- If `force=true` is unsupported for that proposal, return 409 or 422 with standard error shape

### 4. Reject Proposal — `POST /v1/proposals/{id}/reject`
Request:
```json
{
  "archive": true
}
```
Response:
```json
{
  "id": "prop_01HZZXYZ",
  "rejected": true,
  "status": "rejected",
  "archived": true,
  "resolved_at": 1741977665
}
```

### 5. Permission Prompt Response — `POST /v1/permissions/prompts/{id}/respond`
Request:
```json
{
  "decision": "allow",
  "scope": "session"
}
```
Response:
```json
{
  "id": "perm_01HZZABC",
  "resolved": true,
  "decision": "allow",
  "scope": "session",
  "status": "approved",
  "message": "Permission granted for this session.",
  "resumed_stream": true
}
```
**Notes:**
- `decision`: `allow` | `deny`
- `scope`: `once` | `session`
- Permission prompts expire after **5 minutes**. After expiry, this endpoint should return **409 Conflict** with the terminal expired/denied state.
- Prompt responses should be safe to retry on duplicate taps; the server should return the resolved state rather than creating duplicate side effects.

### 6. List Persistent Permissions — `GET /v1/permissions`
```json
{
  "preset": "safe",
  "permissions": [
    {
      "tool": "web_search",
      "title": "Web Search",
      "default_level": "ask",
      "effective_level": "ask",
      "skills": ["brave-search"]
    },
    {
      "tool": "file_read",
      "title": "File Read",
      "default_level": "ask",
      "effective_level": "ask",
      "skills": ["workspace-tools"]
    },
    {
      "tool": "shell_command",
      "title": "Shell Commands",
      "default_level": "ask",
      "effective_level": "ask",
      "skills": ["terminal"]
    }
  ],
  "total": 3
}
```
**Notes:**
- `default_level` / `effective_level`: `allow` | `ask` | `deny`
- `skills` is presentation metadata so the UI can group tools under relevant skills

### 7. Update Persistent Permissions — `PATCH /v1/permissions`
Request:
```json
{
  "preset": "power-user",
  "changes": [
    {
      "tool": "web_search",
      "level": "allow"
    },
    {
      "tool": "shell_command",
      "level": "ask"
    }
  ]
}
```
Response:
```json
{
  "updated": true,
  "preset": "power-user",
  "changed_tools": [
    "web_search",
    "shell_command"
  ]
}
```
**Notes:**
- Partial patch only; omitted tools remain unchanged
- Phase 4 preset/config migration should map existing safety settings into this per-tool model on first launch of a Phase 5-capable backend, with the resulting effective preset returned here rather than inferred client-side.

### 8. Start Provider OAuth — `GET /v1/auth/{provider}/oauth-start`
```json
{
  "provider": "openai",
  "authorize_url": "https://auth.openai.com/oauth/authorize?...",
  "flow_token": "oauth_flow_01HZZPKCE",
  "code_challenge_method": "S256",
  "redirect_uri": "fawx-auth://openai/callback"
}
```
**Notes:**
- The backend keeps PKCE verifier/state server-side and returns a short-lived `flow_token` to the app. This is the recommended and preferred architecture for Phase 5.
- The app launches `ASWebAuthenticationSession` with `authorize_url` and registers the `fawx-auth://` callback scheme.

### 9. Complete Provider OAuth — `POST /v1/auth/{provider}/oauth-callback`
Request:
```json
{
  "code": "auth_code_from_redirect",
  "flow_token": "oauth_flow_01HZZPKCE"
}
```
Response:
```json
{
  "provider": "openai",
  "status": "authenticated",
  "auth_method": "oauth",
  "model_count": 12,
  "verified": true,
  "refresh_capable": true
}
```

### 10. Refresh Provider OAuth Token — `POST /v1/auth/{provider}/refresh`
Response:
```json
{
  "provider": "openai",
  "refreshed": true,
  "status": "authenticated",
  "expires_at": 1741981200
}
```
**Notes:**
- In normal operation the engine should call this transparently before expiry.
- If refresh fails, return an auth-required error state so the client can prompt for re-authentication.

### 11. Search Marketplace Skills — `GET /v1/skills/search?q=portfolio`
```json
{
  "query": "portfolio",
  "skills": [
    {
      "name": "portfolio-tracker",
      "title": "Portfolio Tracker",
      "description": "Track holdings, prices, and portfolio snapshots.",
      "publisher": "fawx-ai",
      "signed": true,
      "tools": ["web_fetch", "file_write"],
      "installed": false
    }
  ],
  "total": 1
}
```
**Notes:**
- Backend proxies/normalizes marketplace API responses

### 12. Install Marketplace Skill — `POST /v1/skills/install`
Request:
```json
{
  "name": "portfolio-tracker"
}
```
Response:
```json
{
  "installed": true,
  "skill": {
    "name": "portfolio-tracker",
    "title": "Portfolio Tracker",
    "source": "marketplace",
    "signed": true,
    "tools": ["web_fetch", "file_write"]
  }
}
```

### 13. Remove Installed Skill — `DELETE /v1/skills/{name}`
Response:
```json
{
  "name": "portfolio-tracker",
  "removed": true
}
```

### 14. Usage Summary — `GET /v1/usage`
```json
{
  "currency": "USD",
  "today": 2.14,
  "week": 11.82,
  "month": 34.09,
  "burn_warning": {
    "active": true,
    "level": "warning",
    "message": "Usage is higher than your recent average today.",
    "baseline_window_days": 7,
    "threshold_multiplier": 2.0
  },
  "updated_at": 1741977600
}
```
**Notes:**
- `burn_warning.level` suggested values: `info` | `warning` | `critical`

### 15. Get Synthesis — `GET /v1/synthesis`
```json
{
  "synthesis": "Be concise, opinionated, and prioritize actionable answers.",
  "updated_at": 1741977600,
  "source": "settings",
  "version": "syn_42",
  "max_length": 4000
}
```
**Notes:**
- `source` could later help explain whether the last change came from chat vs settings
- `version` (or ETag equivalent) exists so the client can detect stale writes

### 16. Set Synthesis — `PUT /v1/synthesis`
Request:
```json
{
  "synthesis": "Be more concise and ask fewer clarifying questions.",
  "version": "syn_42"
}
```
Response:
```json
{
  "updated": true,
  "synthesis": "Be more concise and ask fewer clarifying questions.",
  "updated_at": 1741977660,
  "version": "syn_43"
}
```

### 17. Clear Synthesis — `DELETE /v1/synthesis`
Response:
```json
{
  "cleared": true,
  "version": "syn_44"
}
```

### 18. Exchange Remote Pairing Code — `POST /v1/pair/exchange`
Request:
```json
{
  "host": "my-vps.tailnet.ts.net",
  "pairing_code": "PAIR-7F2K-91LM"
}
```
Response:
```json
{
  "connected": true,
  "server": {
    "host": "my-vps.tailnet.ts.net",
    "display_name": "Joe's VPS",
    "url": "https://my-vps.tailnet.ts.net:8400"
  },
  "auth": {
    "bearer_token_present": true
  }
}
```
**Notes:**
- This request is sent to the **remote host identified by `host`**. It is not a localhost bootstrap helper.
- Response confirms the client can now persist/store the resulting remote credential without ever displaying it

### 19. SSE Permission Prompt Event

The existing SSE stream should add a prompt event for inline permissions. Example wire format:

```text
event: permission_prompt
data: {"id":"perm_01HZZABC","tool":"web_search","title":"Web Search","reason":"Search the web for recent PKCE examples","request_summary":"swift pkce oauth examples","session_scoped_allow_available":true,"expires_at":1741977900}
```

**Notes:**
- Stable `id` is required so the client can answer with `POST /v1/permissions/prompts/{id}/respond`.
- `expires_at` communicates the 5-minute TTL for inline prompts so the UI can show a countdown or expiry state without inventing its own timer semantics.
- When this event is emitted mid-turn, the agent stream is paused pending response/expiry; the client should render the prompt inline and keep the SSE connection open.
- This should be treated like any other SSE event: append a structured card into the transcript.

### 20. Fleet Overview — `GET /v1/fleet/overview`
```json
{
  "total_nodes": 4,
  "healthy_nodes": 3,
  "degraded_nodes": 1,
  "offline_nodes": 0,
  "active_tasks": 7,
  "queued_tasks": 2,
  "updated_at": 1741977600
}
```

### 21. List Fleet Nodes — `GET /v1/fleet/nodes`
```json
{
  "nodes": [
    {
      "id": "node_macmini",
      "name": "Joe's Mac Mini",
      "status": "healthy",
      "last_seen_at": 1741977590,
      "active_tasks": 2,
      "capabilities": ["macos", "build", "gpu"]
    },
    {
      "id": "node_vps1",
      "name": "Primary VPS",
      "status": "degraded",
      "last_seen_at": 1741977500,
      "active_tasks": 5,
      "capabilities": ["linux", "server"]
    }
  ],
  "total": 2
}
```
**Notes:**
- `status` suggested values: `healthy` | `degraded` | `offline`

### 22. Get Fleet Node Detail — `GET /v1/fleet/nodes/{id}`
```json
{
  "id": "node_macmini",
  "name": "Joe's Mac Mini",
  "status": "healthy",
  "last_seen_at": 1741977590,
  "active_tasks": 2,
  "queued_tasks": 1,
  "capabilities": ["macos", "build", "gpu"],
  "recent_tasks": [
    {
      "id": "task_123",
      "title": "Run experiment batch 4",
      "status": "running"
    }
  ]
}
```

### 23. Remove Fleet Node — `DELETE /v1/fleet/nodes/{id}`
Response:
```json
{
  "id": "node_macmini",
  "removed": true
}
```

### 24. Dispatch Fleet Task — `POST /v1/fleet/nodes/{id}/tasks`
Request:
```json
{
  "task": "Run proof-of-fitness tournament on experiment exp_42",
  "priority": "normal"
}
```
Response:
```json
{
  "accepted": true,
  "task_id": "task_999",
  "node_id": "node_macmini",
  "status": "queued"
}
```

### 25. List Experiments — `GET /v1/experiments`
```json
{
  "experiments": [
    {
      "id": "exp_42",
      "name": "Prompt tournament v3",
      "status": "running",
      "score_summary": "leader: chain-b",
      "created_at": 1741977000
    }
  ],
  "total": 1
}
```

### 26. Create Experiment — `POST /v1/experiments`
Request:
```json
{
  "name": "Prompt tournament v3",
  "kind": "proof_of_fitness",
  "config": {
    "population": 16,
    "rounds": 4
  }
}
```
Response:
```json
{
  "id": "exp_42",
  "created": true,
  "status": "queued"
}
```

### 27. Get Experiment Detail — `GET /v1/experiments/{id}`
```json
{
  "id": "exp_42",
  "name": "Prompt tournament v3",
  "kind": "proof_of_fitness",
  "status": "running",
  "created_at": 1741977000,
  "started_at": 1741977060,
  "fleet_nodes": ["node_macmini", "node_vps1"],
  "progress": {
    "completed_matches": 18,
    "total_matches": 32
  }
}
```

### 28. Get Experiment Results — `GET /v1/experiments/{id}/results`
```json
{
  "id": "exp_42",
  "status": "running",
  "leaders": [
    {
      "chain_id": "chain-b",
      "score": 91.2
    },
    {
      "chain_id": "chain-f",
      "score": 88.7
    }
  ],
  "tournament": {
    "round": 3,
    "remaining_matches": 6
  }
}
```

### 29. Stop Experiment — `POST /v1/experiments/{id}/stop`
Response:
```json
{
  "id": "exp_42",
  "stopping": true
}
```
