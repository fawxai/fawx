# Codex Phase 4 Swift Implementation Prompt

**Target:** macOS + iOS Swift app in `app/`  
**Visual reference:** `docs/design/cowork-mockups-p4p5.html` + `docs/design/screenshots/s*.png`  
**API spec:** `docs/specs/phase4-self-contained-install.md` Appendix C  
**Existing app:** `app/Fawx/` — Phase 1-3 already built (chat, sessions, settings, skills)

---

## Architecture Change

**Before Phase 4:** The app connects to an already-running Fawx server. Server lifecycle is external.

**After Phase 4:** The app IS the install. On first launch, it runs a setup wizard, installs a LaunchAgent to run the server as a background daemon, and then acts as a client to that local server.

**Key rule:** The GUI app is purely a client + LaunchAgent manager. It is NEVER the process parent of the Fawx server.

---

## New Files to Create

### Setup Wizard
- `app/Fawx/Views/Shared/SetupWizard/SetupWizardView.swift` — container with step navigation
- `app/Fawx/Views/Shared/SetupWizard/WelcomeStep.swift` — Screen 1
- `app/Fawx/Views/Shared/SetupWizard/TailscaleStep.swift` — Screen 2
- `app/Fawx/Views/Shared/SetupWizard/ProviderStep.swift` — Screen 3
- `app/Fawx/Views/Shared/SetupWizard/ReadyStep.swift` — Screen 4
- `app/Fawx/ViewModels/SetupViewModel.swift` — wizard state machine

### Menu Bar (macOS only)
- `app/Fawx/Views/macOS/MenuBarManager.swift` — NSStatusItem + menu
- `app/Fawx/Views/macOS/MenuBarView.swift` — dropdown content

### Settings Additions
- `app/Fawx/Views/Shared/ServerSettingsPanel.swift` — LaunchAgent controls
- `app/Fawx/Views/Shared/PairingSettingsPanel.swift` — QR code + connection info

---

## Files to Modify

- `app/Fawx/FawxApp.swift` — add first-launch detection, show wizard vs main app
- `app/Fawx/ViewModels/AppState.swift` — add `isFirstLaunch`, `setupComplete`, LaunchAgent status
- `app/Fawx/Networking/FawxClient.swift` — add methods for new endpoints
- `app/Fawx/Views/macOS/SettingsView.swift` — add Server and Pairing sections
- `app/Fawx/Views/iOS/iOSSettingsView.swift` — add Server and Pairing sections

---

## Screen → Endpoint Wiring

### Screen 1: Welcome
- No API calls. Static content.
- "Get started" advances to Screen 2.
- Detect first launch: check if `FawxClient` can reach server. If not → show wizard.

### Screen 2: Tailscale Setup
- **On appear:** `GET /v1/setup/status` → read `tailscale.installed`, `tailscale.running`, `tailscale.logged_in`, `tailscale.cert_ready`
- **States:**
  - Not installed → show download link (open `https://tailscale.com/download`)
  - Installed but not logged in → show "Run tailscale login" guidance
  - Running + logged in → show ✅, auto-run cert:
    - `POST /v1/tailscale/cert` with detected hostname
  - Cert ready → show ✅ ✅, enable Continue
- **Skip** always available → advance to Screen 3

### Screen 3: Add AI Provider
- **On appear:** `GET /v1/setup/status` → read `auth.providers_configured` to show existing ✅ badges
- **Claude subscription flow ("Sign in with Anthropic"):**
  - User picks Claude → "I have a subscription" → show "Sign in with Anthropic" button
  - Button opens browser: `https://console.anthropic.com/settings/keys`
  - UI shows: "Generate a setup token in the Anthropic console and paste it below"
  - User pastes setup token → `POST /v1/auth/anthropic/setup-token` with `{ "setup_token": "<pasted>" }`
  - Show result: authenticated ✅ or error
  - Note: This is token-based auth presented as a sign-in flow. Real OAuth deferred to Phase 5.
- **API key flow (Claude or OpenAI):**
  - User picks provider → "I have an API key" → show key paste field
  - User pastes key → `POST /v1/auth/{provider}/api-key` with `{ "api_key": "<pasted>" }`
  - Never echo key back. Show: authenticated ✅ or error
- **Verify:** `POST /v1/auth/{provider}/verify` with `{ "timeout_seconds": 10 }`
- **Skip** always available

### Screen 4: You're Ready
- **Auto-start toggle:**
  - Toggle ON → `POST /v1/launchagent/install` with `{ "auto_start": true }`
  - Toggle OFF → `POST /v1/launchagent/uninstall`
  - Read current: `GET /v1/launchagent/status` → `installed`, `loaded`
- **QR code (macOS only):**
  - `GET /v1/pair/qr` → `scheme_url`, `display_host`, `port`, `transport`
  - If `transport == "tailscale_https"` → show QR prominently
  - If `transport == "lan_http"` → show QR with warning: "Same network only"
  - If no connectivity → hide QR, show "Set up Tailscale in Settings"
- **"Start chatting"** → dismiss wizard, show main chat view
- **Mark setup complete:** save flag to UserDefaults

### Screen 5: Menu Bar (macOS)
- **Status icon:** poll `GET /v1/server/status` every 10s
  - `status == "running"` → 🟢
  - `status == "stopped"` → 🔴
  - `status == "starting"` → 🟡
- **Menu items:**
  - "Open Fawx" → `NSApp.activate(ignoringOtherApps: true)`, bring window front
  - "Restart Server" → `POST /v1/server/restart`
  - "Stop Server" → `POST /v1/server/stop` (bootout LaunchAgent + SIGTERM — server stays dead until manually started)
  - "Quit" → `NSApp.terminate(nil)` (GUI only, server continues if LaunchAgent active)
  - "Stop Server & Quit" → `POST /v1/server/stop` then `NSApp.terminate(nil)`

### Screen 6: Server Settings (in Settings)
- **Server status:** `GET /v1/server/status` → show status dot + uptime
- **Auto-start toggle:** `GET /v1/launchagent/status` → toggle
  - ON → `POST /v1/launchagent/install`
  - OFF → `POST /v1/launchagent/uninstall`
- **Port field:** read from `GET /v1/server/status` → `port`
  - On change → `PATCH /v1/config` with `{ "changes": { "http": { "port": <new> } } }`
  - If `restart_required` in response → prompt user to restart
- **Restart button:** `POST /v1/server/restart`
- **Stop button:** `POST /v1/server/stop`

### Screen 7: iPhone Pairing (in Settings)
- **macOS:** `GET /v1/pair/qr` → display QR code + connection info
  - "Regenerate" → call endpoint again
  - Show transport badge: green "Tailscale HTTPS" or yellow "Local Network"
- **iOS:** Show connection status to paired Mac, NOT a QR code
  - If paired: show server hostname, status, "Disconnect" button
  - If not paired: show "Scan QR Code" button → open camera

---

## Navigation Flow

```
App Launch
├── First launch (no UserDefaults flag) → SetupWizardView
│   ├── WelcomeStep → TailscaleStep → ProviderStep → ReadyStep
│   └── On "Start chatting" → set flag, show main app
├── Server unreachable → ConnectionBannerView (existing)
└── Normal → existing chat/sessions UI
```

### macOS-specific:
- Menu bar icon persists after window close
- Closing window ≠ quitting app (set `NSApp.setActivationPolicy(.accessory)` when window closes)
- Menu bar creates `NSStatusItem` in `FawxApp.swift` init

### iOS-specific:
- **Simplified pairing wizard, NOT a server setup wizard.** iPhone doesn't run a server — it pairs to an existing Mac.
- iOS wizard flow: Welcome → "Connect to Fawx" → scan QR code or enter pairing code → Done
- No Tailscale step (Mac handles that), no provider step (Mac handles that), no LaunchAgent step
- Settings shows connection status (connected to X, running/stopped) — view-only, not management
- Server Settings panel on iOS is read-only status, not restart/stop controls

---

## Design System (match exactly)

Already in the codebase at `app/Fawx/Theme/`:
- `Colors.swift` — `fawxAccent`, `fawxBackground`, `fawxSurface`, `fawxText`, etc.
- `Typography.swift` — `heading1`, `heading2`, `chatBody`, etc.
- `Spacing.swift` — `paddingSM`, `paddingMD`, `paddingLG`, etc.

Use these existing theme values. Do not hardcode colors or sizes.

---

## Implementation Order

1. **SetupWizardView + WelcomeStep** — minimal, gets the navigation shell working
2. **TailscaleStep** — calls `/v1/setup/status`, displays states
3. **ProviderStep** — calls auth endpoints, token paste flow
4. **ReadyStep** — LaunchAgent install + QR display
5. **MenuBarManager** (macOS) — NSStatusItem + polling
6. **Server/Pairing settings panels** — extend existing settings
7. **First-launch detection** in FawxApp.swift
8. **iOS pairing flow** (scan QR, not display QR)

---

## Testing

- UI tests for setup wizard flow (advance through steps, skip, go back)
- Test first-launch detection (flag absent → wizard, flag present → main app)
- Test server status polling (mock responses for running/stopped/starting)
- Test auth endpoint calls (verify token never echoed)
- Test LaunchAgent install/uninstall toggle
