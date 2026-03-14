# Fawx Native App — Phase 4 Self-Contained Install Specification

**Status:** DRAFT
**Phase:** 4 — Self-Contained Install
**Target:** macOS (Apple Silicon only for V1), with iPhone pairing/connection updates for the existing iOS client
**Minimum OS:** macOS 14 Sonoma
**Distribution:** Notarized DMG (direct download, outside Mac App Store)
**Primary Goal:** A Mac user installs Fawx, completes setup, and starts chatting without ever touching the terminal.

---

## Table of Contents

1. [Product Goal](#1-product-goal)
2. [Scope and Non-Goals](#2-scope-and-non-goals)
3. [Platform and Distribution Decisions](#3-platform-and-distribution-decisions)
4. [Packaging, Build, and Notarization](#4-packaging-build-and-notarization)
5. [First Launch Experience](#5-first-launch-experience)
6. [Setup Wizard](#6-setup-wizard)
7. [Server Lifecycle and LaunchAgent Architecture](#7-server-lifecycle-and-launchagent-architecture)
8. [Menu Bar Experience](#8-menu-bar-experience)
9. [iPhone Pairing and Remote Connectivity](#9-iphone-pairing-and-remote-connectivity)
10. [Backend API Requirements](#10-backend-api-requirements)
11. [Configuration Presets](#11-configuration-presets)
12. [Updates, Uninstall, Logs, and Operations](#12-updates-uninstall-logs-and-operations)
13. [Privacy, Accessibility, and Trust](#13-privacy-accessibility-and-trust)
14. [CLI Setup Wizard Alignment](#14-cli-setup-wizard-alignment)
15. [Implementation Plan](#15-implementation-plan)
16. [Open Questions](#16-open-questions)
17. [Appendix A: Build Pipeline](#appendix-a-build-pipeline)
18. [Appendix B: Phase 4 Decision Summary](#appendix-b-phase-4-decision-summary)
19. [Appendix C: Phase 4 API Schemas](#appendix-c-phase-4-api-schemas)

---

## 1. Product Goal

Phase 4 turns the Fawx Swift app from a client for an already-configured server into a fully self-contained Mac install.

The user journey should be:
1. Download Fawx from `fawx.ai/download`
2. Open a standard macOS DMG
3. Drag Fawx into Applications
4. Launch the app
5. Complete a guided setup wizard
6. Start chatting immediately
7. Optionally pair iPhone via QR code

The defining requirement for this phase is simple:

> **A normal Mac user should never need to open Terminal to install, configure, launch, update, or use Fawx.**

This phase also establishes the long-term product shape: the GUI is the primary user-facing experience, while the Fawx server continues to exist as the local engine underneath.

---

## 2. Scope and Non-Goals

### In Scope
- Self-contained macOS app bundle with embedded Fawx server binary
- Direct-download DMG distribution
- Apple code signing and notarization
- First-launch install detection and setup wizard
- Tailscale-first secure remote connectivity flow
- AI provider onboarding for Claude and ChatGPT
- LaunchAgent-managed background server lifecycle
- Menu bar status/control surface
- QR-based iPhone pairing
- Auto-update framework integration
- Full uninstall/data removal flow
- VoiceOver and accessibility support
- Logging visibility in the GUI

### Out of Scope for Phase 4
- Mac App Store distribution
- Intel/x86_64 build in V1
- Bonjour/LAN auto-discovery
- Multi-server management or fleet UX
- OpenRouter onboarding
- ChatGPT subscription OAuth flow
- EULA / Terms of Service ship work
- Any requirement that the GUI directly parents the server process

### Product Positioning
This phase intentionally follows the distribution model used by tools like Docker Desktop, Tailscale, and VS Code: a signed, notarized direct-download app with system-level capabilities that would be blocked or heavily constrained by App Store sandboxing.

---

## 3. Platform and Distribution Decisions

### 3.1 Supported Hardware and OS

- **DECIDED:** V1 ships as **ARM / Apple Silicon only**.
- **DECIDED:** Minimum supported OS is **macOS 14 Sonoma**.
- **OPEN:** Whether to publish a separate Intel build later depends on real user demand.

### 3.2 Distribution Channel

- **DECIDED:** Fawx ships as a **notarized DMG**, not through the Mac App Store.
- **DECIDED:** Users download from `fawx.ai/download`.

### 3.3 Why Not the Mac App Store

- **DECIDED:** Mac App Store distribution is not viable for Phase 4 because App Sandbox restrictions conflict with core Fawx requirements.

Blocked or constrained capabilities include:
- Binding to local ports
- Running/managing background server processes
- Installing and controlling LaunchAgents
- Reading and writing `~/.fawx/`

This is a product architecture decision, not just a release preference. Fawx needs host-level behavior that is normal for a direct-download Mac utility but hostile to App Store sandboxing.

### 3.4 DMG UX

- **DECIDED:** DMG uses the standard drag-to-Applications layout.
- **DECIDED:** Contents are:
  - Fawx app icon
  - Arrow indicator
  - Applications folder alias
- **DECIDED:** Estimated download size is **30–50 MB**.

### 3.5 Embedded Binary Layout

- **DECIDED:** The Rust server binary is bundled inside the app at:

```text
Fawx.app/Contents/MacOS/fawx-server
```

- **DECIDED:** The app runs that embedded binary directly.
- **DECIDED:** Server binary updates ship with app updates.

Implementation note:
- The Swift app must never rely on a separately installed `fawx` CLI binary for Phase 4 core flows.
- If the CLI remains available for power users, it is an optional interface, not a product dependency.

---

## 4. Packaging, Build, and Notarization

### 4.1 Build Pipeline

**DECIDED:** Phase 4 packaging pipeline is:

```text
cargo build --release --target aarch64-apple-darwin    (Fawx server binary)
         ↓
xcodebuild  (Swift app, embeds binary in .app/Contents/MacOS/)
         ↓
codesign --deep --sign "Developer ID Application: ..."
         ↓
create-dmg → Fawx.dmg
         ↓
xcrun notarytool submit + staple
         ↓
Upload to fawx.ai/download
```

### 4.2 Signing and Notarization

- **DECIDED:** Apple Developer account is already available.
- **DECIDED:** Build uses a **Developer ID Application** certificate.
- **DECIDED:** `codesign` signs both:
  - the `.app` bundle
  - the embedded `fawx-server` binary
- **DECIDED:** Notarization uses:
  - `xcrun notarytool submit`
  - `xcrun stapler staple`
- **DECIDED:** Notarization is expected to take **2–5 minutes per build**.
- **DECIDED:** The process must be fully automatable in CI/release tooling.

Implementation notes:
- The embedded binary must be present before final app signing.
- Signing order matters: nested content should be signed before or as part of the deep sign step used for final packaging.
- Release automation should fail hard on any notarization rejection; shipping an unstapled DMG is not acceptable for this phase.

### 4.3 Operational Requirement

The release artifact is not considered shippable unless all of the following are true:
- DMG opens without security warnings beyond normal macOS notarized-app confirmation
- App launches cleanly on a fresh Apple Silicon Mac running Sonoma
- Embedded server binary executes from inside the bundle
- Notarization ticket is stapled successfully

---

## 5. First Launch Experience

### 5.1 Existing Install Detection

On first launch, the app determines whether the user is in **local-server mode** or **remote-client mode**.

- **DECIDED:** Detection condition for local mode is: `~/.fawx/` exists **and** contains a valid config, or the local LaunchAgent/server is already running.
- **DECIDED:** If an existing valid local install is found, show:
  - **"Found existing Fawx installation. Use existing config?"**
- **DECIDED:** Accepting this skips the Phase 4 setup wizard, starts or reconnects to the local server, and auto-connects the GUI to localhost using the generated local bearer token.
- **DECIDED:** If no valid local install exists, launch the Phase 4 setup wizard.
- **DECIDED:** The Phase 1–3 remote connection flow remains supported for users who want to use the app purely as a client to another Mac running Fawx.
- **DECIDED:** If the user chooses remote mode, the app shows the existing server URL + bearer token onboarding flow from `swift-app-spec.md` Section 5.0.

Implementation notes:
- “Valid config” means parseable config with enough required data for local server startup, not merely directory existence.
- If `~/.fawx/` exists but config is corrupted or incomplete, the app should not silently trust it. That case routes into wizard/recovery UX.
- Shipping Phase 4 does not remove the existing remote-client architecture; it adds a new local-server onboarding path alongside it.

### 5.2 Firewall Prompt Preparation

When the server first binds a listening port, macOS may show the incoming connections firewall prompt.

- **DECIDED:** The setup wizard must show a heads-up *before* the OS dialog appears.
- **DECIDED:** Copy:

> **Fawx needs network access so your iPhone can connect and so it can reach AI providers. macOS will ask you to allow this — tap Allow.**

This is a trust-building requirement. The user should understand *why* the OS is asking before the system prompt appears.

### 5.3 Port Selection Strategy

- **DECIDED:** Try port **8400** first.
- **DECIDED:** If 8400 is occupied, automatically try **8401–8410**.
- **DECIDED:** Store the chosen port in config.
- **DECIDED:** The Swift app must never hardcode the port after this point; it always reads from config or discovery.
- **DECIDED:** The iPhone pairing QR code encodes whichever port was selected.

Implementation notes:
- Port selection must happen before LaunchAgent startup is finalized.
- Port discovery should be deterministic and race-aware; if a port appears free but bind fails, continue to the next candidate.
- All local connection UI should treat the configured port as the source of truth.

### 5.4 First-Run Mode Selection and Coexistence With Phase 1–3

Phase 4 adds a local-server installation path, but it does **not** remove the existing remote-server client flow from the Swift app spec. The product supports two onboarding modes:

1. **Local server (Phase 4 path)**
   - The user installs the Mac app, completes the setup wizard, and Fawx configures the embedded local server.
   - The app then auto-connects to `localhost` using the locally generated bearer token from the Fawx config.
   - This is the default path when no existing remote-only setup is present and the user wants Fawx to run on this Mac.

2. **Remote server (Phase 1–3 path)**
   - The user skips local setup and enters a server URL + bearer token manually, exactly as defined in `swift-app-spec.md` Section 5.0.
   - This is for users who already run Fawx on another Mac, VM, or always-on machine and want this app to behave purely as a remote client.

**Mode detection on first launch:**
- If a local LaunchAgent/server is already running or a valid local config exists, the app assumes **local mode** and offers to reuse that install.
- Otherwise, the default first-run experience is the Phase 4 setup wizard, with a clearly labeled escape hatch such as **“Connect to another Fawx server instead”** for remote mode.
- If a user previously completed remote onboarding, shipping Phase 4 must not force them back through local setup. Their saved server URL + token remain valid and the app opens in remote-client mode until they explicitly opt into local install.

Implementation note:
- The app should persist the last selected connection mode so subsequent launches are deterministic, while still allowing the user to switch modes later from Settings.

---

## 6. Setup Wizard

### 6.1 Role of the Wizard

The setup wizard is the primary user-facing configuration interface for Fawx.

- **DECIDED:** In CLI, the answer is always `fawx setup`.
- **DECIDED:** In GUI, Settings includes a **Setup Wizard** button that reopens the same flow.
- **DECIDED:** The wizard is a reusable checklist flow, not a one-time onboarding throwaway.

Product rule:
- “Something wrong with your config? Run `fawx setup.`”
- “First time? Run `fawx setup.`”
- “Adding a new provider? Run `fawx setup.`”

### 6.2 Core Design Principles

These are hard product rules, not just design guidance.

1. **DECIDED — Skip is always safe.**
   - Every step has a visible **Skip** / **Not now** action.
   - Skipping writes nothing.
   - If config already exists, skip leaves it untouched.
   - If no config exists, skip proceeds without inventing defaults.

2. **DECIDED — Changing is non-destructive.**
   - Existing configuration is shown before editing.
   - Backing out or skipping does not modify current values.
   - Only explicit **Save** writes changes.

3. **DECIDED — State is visible.**
   - Each step shows one of:
     - ✅ **Configured** — current value shown — tap to change
     - ⚠️ **Not configured** — tap to add
     - ⏭️ **Skipped** — “You can set this up later in Settings”

4. **DECIDED — No silent overwrites.**
   - If a change would modify an existing value, show old → new before saving.

5. **DECIDED — No required ordering.**
   - Steps behave like a checklist, not a strict pipeline.

6. **DECIDED — Clear exit language.**
   - Use **“Skip for now”** or **“I’ll do this later”**.
   - Do **not** use ambiguous **“Cancel”** for normal step exit.

Anti-pattern explicitly forbidden:
- A wizard that writes blank or partial config over a working install because the user advanced through empty fields.

### 6.3 Screen 1 — Welcome

- **DECIDED:** Keep this screen minimal.
- **DECIDED:** Primary copy:
  - **“Welcome to Fawx. Your self-hosted AI agent.”**
- **DECIDED:** Show a brief value prop and a **Get started** button.
- **DECIDED:** Detailed product education belongs on the website/download page, not in the wizard.

Implementation note:
- This screen should also include the Phase 4 privacy disclosure text described later in the spec.

### 6.4 Screen 2 — Tailscale Setup

Tailscale is the default remote-connectivity path for Phase 4.

- **DECIDED:** This step is part of the default flow, but remains skippable.
- **DECIDED:** Primary framing:
  - **“Fawx uses Tailscale to securely connect your devices.”**

Behavior:
- Detect whether Tailscale is already installed and running.
- If detected:
  - Show success state
  - Skip with confirmation
- If not installed:
  - Guide user to download Tailscale
  - Offer link to App Store or Tailscale website
  - Explain account creation and sign-in flow

Additional rules:
- **DECIDED:** Skipping Tailscale means the recommended secure iPhone pairing flow is unavailable during setup.
- **DECIDED:** User can return later via Settings to complete it.
- **DECIDED:** When Tailscale is detected, Fawx automatically runs `tailscale cert` to configure HTTPS.
- **DECIDED:** This makes iPhone connections secure out of the box with zero extra user effort.

Implementation notes:
- The app should separate “installed” from “installed and authenticated/running.”
- The wizard should not imply that Tailscale is required for local-only Mac use.
- `tailscale cert` has prerequisites: the machine must be logged into Tailscale, MagicDNS must be enabled on the tailnet, and HTTPS certificates must be enabled in the Tailscale admin console.
- Automatic `tailscale cert` should surface success/failure clearly with actionable guidance, e.g. **“Tailscale is installed, but HTTPS certificates are not available for this tailnet yet.”** The user should not need terminal output, but the app must log details for debugging.
- If Tailscale is skipped entirely, the app may still offer LAN-only pairing later as a fallback, but must label it clearly as same-network-only and less robust than the Tailscale path.

### 6.5 Screen 3 — Add AI Provider

#### Provider Choice

- **DECIDED:** Present two large provider buttons:
  - **Claude (Anthropic)**
  - **ChatGPT (OpenAI)**

#### Connection Method Choice

After selecting a provider, show:

> **How do you want to connect?**

Options:
- **I have a subscription**
- **I have an API key**

#### Claude / Anthropic

- **DECIDED:** Claude subscription path uses the existing **setup-token flow**.
- **DECIDED:** The app opens the Anthropic console in the browser.
- **DECIDED:** User generates a token and pastes it back into the app.
- **DECIDED:** Manual API key paste remains available.

#### ChatGPT / OpenAI

- **DECIDED:** ChatGPT subscription PKCE OAuth flow is **deferred to Phase 5**.
- **DECIDED:** For Phase 4, ChatGPT users use the **manual API key** path.

#### Other Providers

- **DECIDED:** OpenRouter is deferred to a later phase.

#### UX Rules

- **DECIDED:** User may skip provider setup entirely.
- **DECIDED:** If skipped, the app still opens but shows:
  - **“Add a provider to start chatting”**
- **DECIDED:** Existing credentials are shown with ✅ status and can be tapped to change.

### 6.6 Model Selection Rule

- **DECIDED:** There is **no model selection screen** in the setup wizard.
- **DECIDED:** Fawx auto-picks the best available model from the provider's currently supported model list.
- **DECIDED:** Initial defaults at the time of writing are:
  - Claude → **Opus 4.6**
  - ChatGPT → **GPT-5.4**
- **DECIDED:** These names are implementation defaults, not long-term hardcoded product guarantees; they may be updated before ship as provider offerings change.
- **DECIDED:** Users can change model later in Settings.

Rationale:
- This removes decision fatigue for non-technical users during onboarding.

### 6.7 Screen 4 — You’re Ready

Final setup screen contents:
- **DECIDED:** Headline: **“Fawx is running on this Mac”**
- **DECIDED:** Auto-start toggle:
  - **“Start Fawx when you log in?”**
  - Enabling installs the LaunchAgent if needed
- **DECIDED:** QR code for iPhone pairing is shown here when pairing details can be generated
- **DECIDED:** Pairing remains accessible later from:
  - Mac Settings
  - iPhone connection screen
- **DECIDED:** Primary CTA is **“Start chatting”**

Conditional QR behavior:
- If Tailscale HTTPS is configured, generate the QR with the Tailscale hostname and HTTPS URL.
- If the user skipped Tailscale, the app may still show a fallback QR that uses the Mac's current LAN IP + configured port, with a warning: **“This only works while your iPhone is on the same local network as this Mac.”**
- If no safe/reachable pairing address can be determined, hide the QR and show guidance instead of a broken code.

Implementation note:
- The “ready” screen should only appear once the app has successfully completed enough setup to start the local server, or has confirmed an existing usable configuration.

---

## 7. Server Lifecycle and LaunchAgent Architecture

### 7.1 Core Architecture

- **DECIDED:** The GUI app does **not** run the server as a subprocess.
- **DECIDED:** Instead, Fawx installs a LaunchAgent plist at:

```text
~/Library/LaunchAgents/ai.fawx.server.plist
```

- **DECIDED:** The LaunchAgent runs the bundled server binary independently of the GUI app.
- **DECIDED:** `KeepAlive` in the plist provides crash restart behavior.
- **DECIDED:** The GUI connects to the already-running server on localhost, just like the iPhone client does.

### 7.2 Lifecycle Consequences

This architecture means:
1. First launch → setup wizard → installs LaunchAgent → server starts
2. User closes app → server keeps running
3. Mac reboots → server auto-starts on login if enabled
4. User opens app later → app reconnects to local server
5. Menu bar UI remains the persistent control surface

### 7.3 Architectural Rule

> **The GUI app is purely a client + LaunchAgent manager. It is never the process parent of the Fawx server.**

This rule matters because it avoids brittle app-lifetime coupling and gives the user predictable “always on” behavior.

### 7.4 LaunchAgent Responsibilities

The LaunchAgent must:
- Start the server on login when enabled
- Keep the server alive across crashes
- Use the selected/configured port
- Run without requiring the GUI window to stay open

Implementation notes:
- The executable path in the plist must reference the bundled server binary inside the `.app`, not a system-installed CLI dependency.
- If configuration values like port or TLS paths change, LaunchAgent management code must update or regenerate the plist safely.
- Restart flows should be idempotent; repeated enable/disable actions should not create duplicate or divergent plist state.
- The embedded Fawx binary is also responsible for generating the bearer token during local setup, writing it into the Fawx config/credential store, and exposing it to the GUI through config/status reads. The user never types or sees this token during normal local onboarding.

### 7.5 Run Without GUI

- **DECIDED:** With LaunchAgent enabled, the server starts at login even if the user never opens the GUI.
- **DECIDED:** After initial setup, the GUI is optional.
- **DECIDED:** This supports power-user workflows where the server stays running and users alternate between GUI and TUI.
- **DECIDED:** User enables this from the setup wizard or Settings.

---

## 8. Menu Bar Experience

### 8.1 Status Presence

- **DECIDED:** Phase 4 includes an always-visible menu bar icon.
- **DECIDED:** Status states are:
  - 🟢 Running
  - 🔴 Stopped
  - 🟡 Starting

### 8.2 Menu Actions

Clicking the menu bar icon opens a dropdown with:
- **Open Fawx** — bring main window to front
- **Restart Server**
- **Stop Server**
- **Quit** — quits the GUI app and menu bar agent only; the server continues running if LaunchAgent auto-start is enabled
- **Stop Server & Quit** — explicitly stops the local server, unloads active app-side control surfaces, and exits the GUI

### 8.3 Window vs Process Behavior

- **DECIDED:** Closing the main app window does not stop the server.
- **DECIDED:** The menu bar icon persists after window close.
- **DECIDED:** **Quit** closes the GUI and menu bar process only. It does **not** shut down the server when LaunchAgent-managed background operation is enabled.
- **DECIDED:** **Stop Server & Quit** is the explicit full-shutdown action for users who want to take the local server offline.

Implementation notes:
- The menu bar icon is the quick operational surface for users who treat Fawx as an always-on local utility.
- Server state shown in the menu bar should come from real health/status checks, not optimistic UI assumptions.
- If the user chooses **Stop Server** or **Stop Server & Quit** while LaunchAgent auto-start remains enabled, the app should make clear whether this is a temporary stop or whether the agent will restart the server automatically on the next login/manual enable cycle.

---

## 9. iPhone Pairing and Remote Connectivity

### 9.1 Pairing QR Code

- **DECIDED:** Pairing uses a custom URL scheme.
- **DECIDED:** Format:

```text
fawx://connect?host=joes-mac.tail1234.ts.net&port=8400&token=<bearer_token>
```

- **DECIDED:** Host uses the Tailscale hostname when Tailscale HTTPS is configured; otherwise fallback pairing may use the current LAN IP with an explicit same-network warning.
- **DECIDED:** QR includes the bearer token so the user never types it.
- **DECIDED:** Target UX is scan → app opens → auto-connects → done in ~3 seconds.

Implementation notes:
- QR generation should come from a backend or shared utility that guarantees host/port/token are synchronized with current config.
- Token-containing QR codes should be treated as sensitive UI; do not persist screenshots or analytics around them.

### 9.2 HTTPS via Tailscale

- **DECIDED:** HTTPS is configured automatically using `tailscale cert` during setup.
- **DECIDED:** iPhone connects using:

```text
https://hostname.tailnet.ts.net:8400
```

- **DECIDED:** This satisfies iOS ATS requirements without custom exceptions or hacks.

### 9.3 Sleep and Availability Model

- **DECIDED:** Tailscale reconnects automatically when the Mac wakes.
- **DECIDED:** If the server is unreachable, iPhone shows:
  - **“Server offline — wake your Mac to reconnect”**
- **DECIDED:** Always-on setups like Mac Mini/desktop rely on LaunchAgent; `caffeinate` is optional.
- **DECIDED:** No special anti-sleep behavior is added for laptops in Phase 4.

### 9.4 Deferred Discovery and Fleet Features

- **DECIDED:** Bonjour is deferred to Phase 5.
- **DECIDED:** Multiple-server support is deferred to Phase 5+ fleet work.
- **DECIDED:** Tailscale-only discovery is sufficient for Phase 4.

---

## 10. Backend API Requirements

Phase 4 requires additional HTTP endpoints so the GUI can perform setup, credential management, pairing, and operational actions without shelling out to the terminal.

### 10.1 New Endpoints

| Endpoint | Method | Purpose |
|---|---|---|
| `/v1/setup/status` | GET | Determine whether Fawx is configured: providers, bearer token, Tailscale status |
| `/v1/auth/anthropic/setup-token` | POST | Exchange Claude setup token for credentials |
| `/v1/auth/{provider}/api-key` | POST | Store a manual API key |
| `/v1/auth/{provider}` | DELETE | Remove a provider’s credentials |
| `/v1/auth/{provider}/verify` | POST | Verify credentials by making a small provider API call |
| `/v1/pair/qr` | GET | Generate QR code payload for pairing |
| `/v1/server/status` | GET | Health, uptime, version; extends current `/health` role |
| `/v1/server/restart` | POST | Trigger server restart |
| `/v1/config` | PATCH | Merge specific config changes into existing config |
| `/v1/config/presets` | GET | List available config presets |
| `/v1/config/preset/{name}` | POST | Apply a preset as an overlay |
| `/v1/config/preset/{name}/diff` | GET | Preview preset changes before apply |
| `/v1/tailscale/cert` | POST | Trigger `tailscale cert` and configure TLS |

### 10.2 Setup, Auth, and Restart Semantics

#### Local bearer token generation

- **DECIDED:** In the Phase 4 local setup flow, the embedded Fawx binary generates the bearer token as part of setup.
- **DECIDED:** The token is written to the normal Fawx config/credential storage alongside the rest of the local server configuration.
- **DECIDED:** The GUI reads the token indirectly through config/status/pairing endpoints and local config access; the user never has to manually copy it in local mode.
- **DECIDED:** The remote-client onboarding flow from the Swift app spec still uses manual URL + token entry, because in that mode the token belongs to some other server.

#### Port exhaustion and subsequent launch behavior

- **DECIDED:** Initial setup scans ports `8400` through `8410`.
- **DECIDED:** If all ports in that range are unavailable, setup enters a terminal error state and does **not** pretend the server is running.
- **DECIDED:** User-facing copy should be explicit, e.g. **“Fawx couldn't start because ports 8400–8410 are all in use. Close one of those apps or change the configured port range, then try again.”**
- **DECIDED:** The wizard remains open, shows troubleshooting guidance, and offers **Retry**.
- **DECIDED:** On subsequent launches, the configured port remains the source of truth. The server does **not** silently hop to a new port, because that would break stored connection settings, QR codes, and iPhone bookmarks.
- **DECIDED:** If the configured port is occupied later, the server fails to start cleanly, the GUI/menu bar show an actionable error, and the user can restart after freeing the port or updating config intentionally.
- **DECIDED:** LaunchAgent KeepAlive may retry, but UI/status surfaces must collapse repeated bind failures into one understandable error state instead of looking like a healthy running server.

#### Concurrent GUI + CLI setup semantics

- **DECIDED:** GUI setup and CLI `fawx setup` operate on the same underlying config model.
- **DECIDED:** Config writes must be serialized with file locking or equivalent single-writer protection.
- **DECIDED:** If one setup flow is active, the other should fail fast with a user-visible **“Setup already in progress”** style error rather than racing.
- **DECIDED:** Last-writer-wins without coordination is explicitly forbidden for setup-critical state such as port, bearer token, and provider credentials.

#### Server restart mechanism

- **DECIDED:** `POST /v1/server/restart` uses the LaunchAgent/KeepAlive model: the server exits gracefully with a restart-specific status path, and launchd brings it back.
- **DECIDED:** The GUI should treat restart as a brief disconnect/reconnect cycle, not as an in-process hot reload.
- **DECIDED:** In-flight requests may fail during restart and should surface a clear transient status to the client.

### 10.3 Endpoint Design Rules

#### `/v1/config` must be merge-only

- **DECIDED:** Config updates are partial overlays, not full replacement writes.
- **DECIDED:** GUI writes must avoid destructive rewrite semantics.

This is a direct consequence of the wizard’s non-destructive design principles.

#### Provider verify endpoint is required

- **DECIDED:** Credential entry is not complete until the app can verify the credentials actually work.
- A small provider API call should validate auth and return a user-friendly result.

#### Server restart endpoint is required

- **DECIDED:** The GUI must be able to restart the server from the menu bar/settings flow without requiring CLI access.

Implementation note:
- Restart behavior must work cleanly with LaunchAgent management and should not leave the app in a stale “running” state during restart transitions.

---

## 11. Configuration Presets

### 11.1 Preset Model

- **DECIDED:** Presets are **overlays**, not full config replacements.
- **DECIDED:** Applying a preset only changes the keys the preset defines.
- **DECIDED:** Unrelated settings remain untouched.

Example:

```yaml
Safe:
  auto_approve_proposals: false
  tool_budget: conservative
  require_confirmation: true
  max_loop_iterations: 5

Power User:
  auto_approve_proposals: true
  tool_budget: generous
  require_confirmation: false
  max_loop_iterations: 20
```

### 11.2 Preservation Rules

- **DECIDED:** Preset changes do **not** touch:
  - API keys
  - model choice
  - port
  - system prompt
  - Tailscale config
  - any other unrelated config keys

### 11.3 Preview / Diff UX

- **DECIDED:** Preset apply flow includes a preview endpoint:
  - `/v1/config/preset/{name}/diff`
- **DECIDED:** Confirmation UI should say something like:

> **Switching to Safe mode will change:**
> `key: old → new`
>
> **Your other settings won’t be affected.**

Buttons:
- **Apply**
- **Cancel**

Implementation notes:
- Presets should be represented in a way the GUI can render cleanly: name, description if available, and a typed list of key/value changes.
- Diff results should be explicit even when values are being newly added rather than changed.

---

## 12. Updates, Uninstall, Logs, and Operations

### 12.1 Auto-Updates

- **DECIDED:** Use the **Sparkle** framework for Mac auto-updates.
- **DECIDED:** Updates are fetched from `fawx.ai` DMG/update infrastructure.
- **DECIDED:** Menu bar dropdown includes **Check for Updates**.

Implementation notes:
- Sparkle is the standard direct-download update path for Mac utilities and matches the product’s non-App-Store distribution strategy.
- Update UX must preserve the self-contained story: users should not manually replace binaries or use a package manager.

### 12.2 Uninstall Model

- **DECIDED:** Dragging the app to Trash removes the app bundle only.
- **DECIDED:** `~/.fawx/` persists by default for data preservation.
- **DECIDED:** The LaunchAgent plist also persists unless explicitly removed. This means trashing the app alone can leave an orphaned `~/Library/LaunchAgents/ai.fawx.server.plist` behind.

Implementation notes:
- If the orphaned plist points at a server binary inside the trashed `.app`, launchd will fail to start it on the next login/retry. This is expected graceful degradation, not silent continued execution from the Trash.
- User-facing uninstall guidance should mention this clearly and steer users toward **Remove all data** for a clean teardown.

### 12.3 Full Data Removal

Settings includes a **Remove all data** action that performs full teardown:
- Stop the server
- Unload the LaunchAgent
- Delete `~/Library/LaunchAgents/ai.fawx.server.plist`
- Delete `~/.fawx/`
- Leave the system fully torn down

Implementation notes:
- This is destructive and must require clear confirmation.
- The uninstall flow should explain the difference between removing the app and removing all app data.
- **Recommendation:** users should run **Remove all data** before dragging the app to Trash, because this is the only flow that can reliably unload and remove the LaunchAgent first.
- If the user drags the app to Trash without doing this, the LaunchAgent plist may remain behind and continue trying to launch the now-missing bundled server binary. The server will fail to start gracefully rather than partially running, and the app should document this as expected degradation until the agent is removed or the app is reinstalled.
- If data removal fails partially, the UI must report which cleanup steps succeeded vs failed.

### 12.4 Settings Information Architecture

Phase 4 substantially expands Settings beyond the original Swift app structure. To avoid ad-hoc UI decisions, Settings should be reorganized into these primary sections:
- **General** — app version, update checks, theme/appearance
- **Connection** — local vs remote mode, server URL, bearer token status, connection test
- **Local Server** — LaunchAgent auto-start, server status, restart/stop controls, port, logs
- **Providers** — Claude/OpenAI credential status, add/change/remove, verification
- **Pairing & Remote Access** — Tailscale state, HTTPS certificate status, QR pairing, LAN fallback guidance
- **Presets & Behavior** — config presets, model defaults, safety/permission-oriented settings that are safe to expose
- **Advanced / Reset** — rerun setup wizard, remove all data, diagnostic links

Implementation note:
- The earlier Swift app Settings sections (Connection, Appearance, Model & Thinking, Auth Status, About) are still conceptually present, but Phase 4 groups them into a broader operational structure suitable for a self-contained desktop product.

### 12.5 Logging

- **DECIDED:** Server logs remain in existing location:

```text
~/.fawx/logs/
```

- **DECIDED:** GUI includes a **View Logs** option in Settings or the menu bar.
- **DECIDED:** This is in scope for Phase 4 because debugging remote connectivity and setup failures requires user-visible diagnostics.

Implementation notes:
- “View Logs” can start as open-in-Finder / open-in-default-viewer if needed; a richer in-app log viewer can evolve later.
- Log access should avoid exposing secrets in UI where possible.

### 12.6 Disk Usage Guidance

For support and user expectation-setting, Phase 4 should document rough storage characteristics for `~/.fawx/`:
- Config + small metadata: typically **< 5 MB**
- Logs: typically **tens of MB**, depending on verbosity and retention
- Conversation history / local data: grows with usage and can become the dominant storage category over time
- TLS assets / cached operational files: typically **small** relative to logs and transcripts

Implementation note:
- The product does not need an exact quota system in Phase 4, but support copy and Settings/help text should make clear that disk usage grows primarily with logs and stored conversations, and that **Remove all data** deletes this local footprint.

---

## 13. Privacy, Accessibility, and Trust

### 13.1 Privacy Disclosure

- **DECIDED:** The welcome screen includes:

> **Your conversations are stored on this Mac. API calls to your AI provider use their standard data handling policies. Connections between your devices use encrypted network paths such as Tailscale or your local network.**

- **DECIDED:** The About page repeats the same disclosure.
- **DECIDED:** This is a core product message and should be treated as a selling point.

This is not just legal copy; it communicates Fawx’s data-sovereignty value proposition.

### 13.2 Accessibility

- **DECIDED:** VoiceOver support is in scope for the setup wizard and the main app.
- **DECIDED:** Accessibility is not deferred.

Implementation notes:
- Wizard steps, buttons, toggles, status indicators, and QR/pairing flows must all have meaningful labels.
- Status states like Configured / Not configured / Skipped must not rely on color alone.

### 13.3 Firewall and Trust Messaging

Phase 4 includes multiple trust-sensitive moments:
- macOS firewall prompt
- Tailscale install/sign-in guidance
- provider credential entry
- bearer-token QR pairing

Implementation note:
- All of these flows should use plain language and explain *why* access is needed before requesting or exposing it.
- On first launch, documentation and download-page copy should also mention the standard macOS Gatekeeper confirmation shown for notarized apps downloaded from the Internet, so the user knows to expect it before the app's own wizard appears.

---

## 14. CLI Setup Wizard Alignment

Phase 4 requires the CLI and GUI setup experiences to share the same philosophy.

### 14.1 CLI Behavior

- **DECIDED:** `fawx setup` becomes the canonical setup command.
- **DECIDED:** CLI setup is interactive, skippable per step, and non-destructive.
- **DECIDED:** It should skip steps that are already configured and allow safe changes.

### 14.2 Removal of Telegram from Setup

- **DECIDED:** Telegram is removed from the setup wizard.
- **DECIDED:** It was a dogfooding channel, not a core end-user setup requirement.
- **DECIDED:** Messaging channel configuration is advanced functionality for config files or a future Integrations area.

Implementation note:
- GUI and CLI should not drift into two different conceptual models. Both should reinforce the same mental model: Fawx setup is a safe checklist you can run anytime.
- Phase 4 supersedes the earlier Swift app limitation that auth management was read-only in the GUI. In Phase 4, credential CRUD and verification are explicitly in scope via backend API + GUI.

---

## 15. Implementation Plan

### 15.1 Workstreams

1. **Packaging and release pipeline**
   - Embed `fawx-server` in app bundle
   - Sign app and nested binary
   - Build DMG
   - Notarize and staple
   - Publish release artifact

2. **First-run and setup state detection**
   - Detect `~/.fawx/` and valid config
   - Detect port conflicts
   - Detect Tailscale installation/running state
   - Detect provider/auth status

3. **Setup wizard UI and state machine**
   - Welcome
   - Tailscale
   - Provider setup
   - Ready / autostart / pairing
   - Re-entry from Settings

4. **LaunchAgent management**
   - Create/update plist
   - Enable/disable autostart
   - Start/stop/restart server
   - Surface menu bar state

5. **Backend API surface**
   - Setup status
   - Credential CRUD + verify
   - QR payload generation
   - Config merge + presets + diff
   - Tailscale cert trigger
   - Server status/restart

6. **Operational polish**
   - Sparkle updates
   - View logs
   - Remove all data
   - Accessibility pass
   - Privacy/trust copy
   - Gatekeeper/download guidance
   - Settings information architecture

### 15.2 Suggested Implementation Order

1. Backend endpoints required to support wizard and settings
2. Embedded binary packaging in app bundle
3. LaunchAgent install/start/stop/restart plumbing
4. First-launch detection and setup status model
5. Setup wizard screens and non-destructive save logic
6. Tailscale detection and cert flow
7. Provider onboarding and credential verification
8. QR pairing generation and ready screen
9. Menu bar controls and status reporting
10. Sparkle, logs, uninstall, accessibility polish

### 15.3 Acceptance Criteria

Phase 4 is complete when a new user on an Apple Silicon Sonoma Mac can:
- Download a notarized DMG
- Install by dragging to Applications
- Launch the app without terminal use
- Complete setup via GUI only
- Add a provider via GUI only
- Choose either local-server mode or remote-client mode on first run
- Start the server and keep it running via LaunchAgent
- Close the window while keeping the server available
- Pair an iPhone via QR code
- Reopen settings/wizard later without destructive config loss
- Update the app via Sparkle
- View logs and fully remove data from the GUI

---

## 16. Open Questions

The planning decisions are mostly settled. Remaining open items are implementation-detail questions rather than product-direction uncertainty.

### OPEN 1 — Existing Config Validity Criteria
What exact fields define a “valid config” for first-launch existing-install detection?

Suggested answer:
- Parseable config file
- Valid HTTP/server section
- Usable port
- Bearer token present or recoverable
- No fatal corruption that prevents startup

### OPEN 2 — LaunchAgent Command Path Strategy
Should the plist invoke the embedded binary directly, or a stable wrapper path inside app support if app location changes after install/update?

This affects robustness if the app bundle is moved after initial LaunchAgent setup.

### OPEN 3 — Tailscale Detection Mechanism
What is the exact implementation strategy for detecting Tailscale installation, login state, and readiness without relying on brittle shell assumptions?

### OPEN 4 — Log Viewer v1 Depth
Should “View Logs” open Finder / Console-friendly files initially, or ship a basic in-app log viewer in Phase 4?

### OPEN 5 — Sparkle Release Feed Details
What exact hosting and signing setup will back Sparkle appcasts and update verification on `fawx.ai`?

### OPEN 6 — ChatGPT API Key UX Wording
Since subscription OAuth is deferred, what is the cleanest wording that explains why ChatGPT subscription users need to use API key setup for now without causing confusion?

## Appendix A: Build Pipeline

```text
cargo build --release --target aarch64-apple-darwin    (Fawx server binary)
         ↓
xcodebuild  (Swift app, embeds binary in .app/Contents/MacOS/)
         ↓
codesign --deep --sign "Developer ID Application: ..."
         ↓
create-dmg → Fawx.dmg
         ↓
xcrun notarytool submit + staple
         ↓
Upload to fawx.ai/download
```

---

## Appendix B: Phase 4 Decision Summary

### Download and Install
- **DECIDED:** Apple Silicon only for V1
- **DECIDED:** Minimum macOS version is 14 Sonoma
- **DECIDED:** Direct-download notarized DMG
- **DECIDED:** Standard drag-to-Applications DMG layout
- **DECIDED:** Estimated size 30–50 MB
- **DECIDED:** Bundle server binary inside `.app/Contents/MacOS/fawx-server`

### First Launch
- **DECIDED:** Detect existing `~/.fawx/` install and offer reuse
- **DECIDED:** Warn user before macOS firewall dialog appears
- **DECIDED:** Auto-select first free port in 8400–8410 and persist it

### Setup Wizard
- **DECIDED:** Skip is always safe
- **DECIDED:** Changes are non-destructive
- **DECIDED:** Config state is always visible
- **DECIDED:** No silent overwrites
- **DECIDED:** No required ordering
- **DECIDED:** Use clear “Skip for now” language
- **DECIDED:** Wizard is primary setup/change interface in GUI and CLI
- **DECIDED:** No model picker in wizard; auto-pick best model
- **DECIDED:** Telegram removed from setup

### Server Lifecycle
- **DECIDED:** GUI is not server parent
- **DECIDED:** LaunchAgent runs the server independently
- **DECIDED:** KeepAlive handles restart on crash
- **DECIDED:** Menu bar persists while server runs
- **DECIDED:** GUI can be optional after initial setup

### Pairing and Connectivity
- **DECIDED:** QR pairing includes host, port, and bearer token
- **DECIDED:** Prefer Tailscale hostname + HTTPS when available
- **DECIDED:** LAN-IP QR fallback is allowed only with a same-network warning
- **DECIDED:** No anti-sleep special cases for laptops
- **DECIDED:** Bonjour deferred to Phase 5
- **DECIDED:** Multi-server deferred to Phase 5+

### Additional Features
- **DECIDED:** Sparkle for updates
- **DECIDED:** Drag-to-trash preserves `~/.fawx/`
- **DECIDED:** GUI includes full data removal option
- **DECIDED:** Privacy disclosure appears on Welcome and About
- **DECIDED:** Accessibility is included in Phase 4 scope
- **DECIDED:** GUI provides log access
- **DECIDED:** EULA / ToS deferred to Phase 6


## Appendix C: Phase 4 API Schemas

All endpoints follow the standard error response format defined in `swift-app-spec.md` Appendix A. Error responses use `{ "error": "<message>" }` with appropriate HTTP status codes (400, 401, 404, 409, 500, 503).

This appendix defines the request/response shapes for the 13 new Phase 4 endpoints. The style intentionally matches `swift-app-spec.md` Appendix A so backend and GUI implementers have one unambiguous contract.

### 1. Setup Status — `GET /v1/setup/status`
```json
{
  "mode": "local",
  "setup_complete": true,
  "has_valid_config": true,
  "server_running": true,
  "launchagent": {
    "installed": true,
    "loaded": true,
    "auto_start_enabled": true
  },
  "local_server": {
    "host": "127.0.0.1",
    "port": 8400,
    "https_enabled": true
  },
  "auth": {
    "bearer_token_present": true,
    "providers_configured": ["anthropic"]
  },
  "tailscale": {
    "installed": true,
    "running": true,
    "logged_in": true,
    "hostname": "joes-mac.tail1234.ts.net",
    "cert_ready": true
  }
}
```
**Notes:**
- `mode`: `"local"` | `"remote"`
- `bearer_token_present` confirms local token generation/storage without exposing the token value
- This endpoint is read-only status, not a mutating setup action

### 2. Exchange Claude Setup Token — `POST /v1/auth/anthropic/setup-token`
Request:
```json
{
  "setup_token": "ast-xxxxxxxx",
  "label": "Personal Claude subscription"
}
```
Response:
```json
{
  "provider": "anthropic",
  "status": "authenticated",
  "auth_method": "setup_token",
  "model_count": 3,
  "verified": true
}
```

### 3. Store Provider API Key — `POST /v1/auth/{provider}/api-key`
Request:
```json
{
  "api_key": "sk-...REDACTED",
  "label": "Work key"
}
```
Response:
```json
{
  "provider": "openai",
  "status": "authenticated",
  "auth_method": "api_key",
  "model_count": 12,
  "verified": false
}
```
**Notes:**
- `provider`: `"anthropic"` | `"openai"` for Phase 4
- Response never echoes the secret value

### 4. Remove Provider Credentials — `DELETE /v1/auth/{provider}`
Response:
```json
{
  "provider": "openai",
  "removed": true
}
```

### 5. Verify Provider Credentials — `POST /v1/auth/{provider}/verify`
Request:
```json
{
  "timeout_seconds": 10
}
```
Response:
```json
{
  "provider": "anthropic",
  "verified": true,
  "status": "authenticated",
  "message": "Credentials verified successfully.",
  "checked_at": 1741977600
}
```

### 6. Pairing QR Payload — `GET /v1/pair/qr`
```json
{
  "scheme_url": "fawx://connect?host=joes-mac.tail1234.ts.net&port=8400&token=REDACTED",
  "display_host": "joes-mac.tail1234.ts.net",
  "port": 8400,
  "transport": "tailscale_https",
  "same_network_only": false
}
```
**Notes:**
- `transport`: `"tailscale_https"` | `"lan_http"`
- If `transport == "lan_http"`, `same_network_only` must be `true` so the GUI can show the warning

### 7. Server Status — `GET /v1/server/status`
```json
{
  "status": "running",
  "version": "0.4.0",
  "uptime_seconds": 3600,
  "pid": 12345,
  "host": "127.0.0.1",
  "port": 8400,
  "https_enabled": true
}
```
**Notes:**
- `status`: `"running"` | `"starting"` | `"stopped"` | `"error"`

### 8. Restart Server — `POST /v1/server/restart`
Response:
```json
{
  "accepted": true,
  "restart_via": "launchagent_keepalive",
  "message": "Server restart requested."
}
```

### 9. Merge Config — `PATCH /v1/config`
Request:
```json
{
  "changes": {
    "http": {
      "port": 8401
    },
    "ui": {
      "auto_start": true
    }
  }
}
```
Response:
```json
{
  "updated": true,
  "restart_required": true,
  "changed_keys": [
    "http.port",
    "ui.auto_start"
  ]
}
```
**Notes:**
- Merge semantics only; omitted keys remain unchanged

### 10. List Presets — `GET /v1/config/presets`
```json
{
  "presets": [
    {
      "name": "safe",
      "title": "Safe",
      "description": "Conservative defaults for cautious use."
    },
    {
      "name": "power-user",
      "title": "Power User",
      "description": "Fewer confirmations, higher autonomy."
    }
  ],
  "total": 2
}
```

### 11. Apply Preset — `POST /v1/config/preset/{name}`
Request:
```json
{
  "confirm": true
}
```
Response:
```json
{
  "name": "safe",
  "applied": true,
  "restart_required": false,
  "changed_keys": [
    "behavior.require_confirmation",
    "behavior.max_loop_iterations"
  ]
}
```

### 12. Preset Diff Preview — `GET /v1/config/preset/{name}/diff`
```json
{
  "name": "safe",
  "changes": [
    {
      "key": "behavior.require_confirmation",
      "old": false,
      "new": true
    },
    {
      "key": "behavior.max_loop_iterations",
      "old": 20,
      "new": 5
    }
  ]
}
```

### 13. Generate Tailscale Certificate — `POST /v1/tailscale/cert`
Request:
```json
{
  "hostname": "joes-mac.tail1234.ts.net"
}
```
Response:
```json
{
  "success": true,
  "hostname": "joes-mac.tail1234.ts.net",
  "cert_path": "~/.fawx/tls/cert.pem",
  "key_path": "~/.fawx/tls/key.pem",
  "https_enabled": true
}
```
**Notes:**
- On failure, use the standard error shape from the Swift app spec Appendix A: `{ "error": "..." }`
- Common failures should map to actionable messages (not logged-only shell output)
