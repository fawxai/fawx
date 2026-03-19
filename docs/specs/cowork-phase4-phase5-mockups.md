# Fawx Phase 4 + Phase 5 — UI Design (macOS + iOS)

**Purpose:** Generate pixel-accurate HTML mockups for all new screens in the Fawx native app, covering Phase 4 (self-contained install) and Phase 5 (full TUI parity). These mockups will be screenshotted and given to Codex as visual references for SwiftUI implementation.

---

## Reference Materials (attach alongside this prompt)

1. **`docs/specs/phase4-self-contained-install.md`** — Self-contained install spec. Sections §5-8 define the setup wizard, server lifecycle, and menu bar. §9 defines iPhone pairing + QR.
2. **`docs/specs/phase5-full-parity.md`** — Full TUI parity spec. §6 defines proposal gate UI, §7 permission system, §10 skill marketplace, §12 cost tracking, §13 synthesis/custom instructions.
3. **`docs/specs/swift-app-spec.md`** — Original app spec with API contracts and existing screen definitions.
4. **`docs/specs/cowork-mockup-prompt.md`** — Previous mockup prompt with the EXACT design system (colors, typography, spacing). **Use these values verbatim.**
5. **`docs/design/cowork-mockups-v2.html`** — Previous mockup output. These screens already exist and work. New screens must match this visual language exactly.

---

## Design System

Use the exact design system from `cowork-mockup-prompt.md` — same colors, typography, spacing, corner radii. Do not deviate. The existing app already uses these values.

### Quick Reference (from cowork-mockup-prompt.md)

| Token | Dark Mode | Light Mode | Usage |
|-------|-----------|------------|-------|
| `background` | `#1A1A1A` | `#FFFFFF` | Main background |
| `surface` | `#242424` | `#F5F5F5` | Cards, sidebar, input bar |
| `surfaceHover` | `#2E2E2E` | `#EBEBEB` | Hover states |
| `surfaceActive` | `#383838` | `#E0E0E0` | Selected/active items |
| `text` | `#E8E8E8` | `#1A1A1A` | Primary text |
| `textSecondary` | `#999999` | `#666666` | Secondary/muted text |
| `accent` | `#E8711A` | `#D45E14` | Fawx orange — buttons, badges, active elements |
| `accentSubtle` | `#E8711A20` | `#D45E1415` | Accent tinted backgrounds |
| `success` | `#4ADE80` | `#22C55E` | Connected, online, passed |
| `warning` | `#FBBF24` | `#D97706` | Reconnecting, caution |
| `error` | `#F87171` | `#DC2626` | Disconnected, errors |
| `border` | `#333333` | `#E5E5E5` | Dividers, card borders |

| Element | Font | Size | Weight |
|---------|------|------|--------|
| Sidebar title | SF Pro | 13px | 600 |
| Chat message body | SF Pro | 14px | 400 |
| Code blocks | SF Mono | 13px | 400 |
| Input bar text | SF Pro | 14px | 400 |
| Status bar | SF Pro | 11px | 400 |
| Heading (H1) | SF Pro | 18px | 700 |
| Heading (H2) | SF Pro | 16px | 600 |

| Token | Value |
|-------|-------|
| `paddingSM` | 8px |
| `paddingMD` | 12px |
| `paddingLG` | 16px |
| `paddingXL` | 24px |
| `cornerRadius` | 8px |
| `sidebarWidth` | 260px (macOS) |
| `maxMessageWidth` | 720px |
| iPhone mockup width | 390px (iPhone 15 Pro) |
| macOS window min | 900×600 |

---

## Screens to Design (17 new screens)

### Phase 4 — Setup Wizard (macOS + iOS versions of each)

#### Screen 1: Welcome
- Centered card, 🦊 emoji (48px), "Welcome to Fawx. Your self-hosted AI agent."
- Brief value prop text (2-3 lines), "Get started" primary button (accent color)
- Privacy disclosure text at bottom (textSecondary, small)
- macOS: centered in window on background. iOS: full-screen card.

#### Screen 2: Tailscale Setup
- Heading: "Fawx uses Tailscale to securely connect your devices."
- Three states to show (as separate variants or with state indicators):
  - **Not installed:** guidance text + "Download Tailscale" link button → opens App Store/website
  - **Installed but not running:** "Start Tailscale" prompt with status indicator
  - **Installed + running:** success checkmark ✅, auto-configure HTTPS via `tailscale cert`, show progress → success
- "Skip" button always visible (secondary style, not prominent)
- Info text: "Skipping means secure iPhone pairing won't be available during setup. You can set it up later in Settings."

#### Screen 3: Add AI Provider
- Two large provider cards side by side (macOS) or stacked (iOS):
  - **Claude (Anthropic)** — Anthropic logo/icon placeholder + name
  - **ChatGPT (OpenAI)** — OpenAI logo/icon placeholder + name
- After selecting a provider, sub-view slides in:
  - Heading: "How do you want to connect?"
  - Two options as large tappable rows:
    - "I have a subscription" → setup-token paste flow
    - "I have an API key" → API key paste field
  - Claude subscription flow: "Open the Anthropic console, generate a setup token, and paste it here." + text input + "Connect" button
  - API key flow: password-style input + "Save" button
  - Response NEVER echoes the key/token back
- Skip button visible
- If credentials already exist: show provider with ✅ badge, option to change

#### Screen 4: You're Ready
- Heading: "Fawx is running on this Mac" (H1)
- Auto-start toggle: "Start Fawx when you log in?" with toggle switch
- QR code section (conditional):
  - **Tailscale HTTPS configured:** large QR code + "Scan with your iPhone to connect"
  - **LAN only:** smaller QR code + warning banner: "This only works while your iPhone is on the same local network as this Mac."
  - **No connectivity:** hide QR, show: "Set up Tailscale in Settings to enable iPhone pairing."
- Primary CTA: "Start chatting" button (accent, full-width on iOS, right-aligned on macOS)

### Phase 4 — Menu Bar (macOS only)

#### Screen 5: Menu Bar Dropdown
- macOS native menu bar icon showing status:
  - 🟢 green dot = Running
  - 🔴 red dot = Stopped
  - 🟡 yellow dot = Starting
- Dropdown menu items (standard macOS menu styling):
  - **Open Fawx** (bold, top item)
  - separator
  - **Restart Server**
  - **Stop Server**
  - separator
  - **Quit** (quits GUI only, server keeps running)
  - **Stop Server & Quit** (explicit full shutdown)
- Show current status at top: "Fawx is running" / "Fawx is stopped"

### Phase 4 — Settings Additions (macOS + iOS)

#### Screen 6: Server & LaunchAgent Settings
- Section within existing Settings screen
- **Server Status:** running/stopped indicator with colored dot
- **Auto-start:** "Start Fawx when you log in" toggle
- **Port:** editable text field showing current port (e.g., 8400)
- **Restart Server** / **Stop Server** buttons (standard secondary style)
- **View Logs** link → opens log file or shows log viewer
- macOS: in existing Settings window as a new section. iOS: in existing Settings list.

#### Screen 7: iPhone Pairing (in Settings)
- Accessible from Settings, separate from setup wizard
- Large QR code in center
- Below QR: connection info text:
  - Hostname: `joes-mac.tail1234.ts.net`
  - Port: `8400`
  - Transport: `Tailscale HTTPS` (green badge) or `Local Network` (yellow badge with warning)
- "Regenerate" button (secondary style)
- If LAN-only: warning banner at top: "Only works on the same local network."
- If no connectivity available: guidance text instead of QR

### Phase 5 — Proposal Gate (macOS + iOS)

#### Screen 8: Inline Proposal Prompt
- Appears during chat, interrupting the message flow
- **iOS:** bottom sheet presentation (not an alert — bottom sheets can't be dismissed accidentally)
- **macOS:** inline card within chat view, or inspector panel
- Header strip colored by tier:
  - Green: Standard (routine operations)
  - Amber: Elevated (system commands, config changes)
  - Red: Sensitive (auth/credentials, TIER2 paths)
- Content:
  - "Fawx wants to **[action verb]** `[target path/resource]`"
  - Full file path displayed (no truncation in detail view)
  - Agent's reason in a distinct visual block: label "Agent's reason:" in textSecondary, reason text below
  - Diff preview section if applicable (collapsed by default, expandable)
- Buttons at bottom:
  - **Deny** (left, default, secondary style)
  - **Approve** (right, primary/accent style)
- For Sensitive tier: "Review Details" button instead of direct Approve → see Screen 9

#### Screen 9: Proposal Diff Viewer (Sensitive tier)
- Modal sheet (iOS) or panel (macOS)
- Full unified diff with syntax highlighting:
  - Green lines: additions
  - Red lines: deletions
  - Gray lines: context
  - Line numbers on left
- File path header at top
- Tier badge (red for Sensitive)
- Approve button has 3-second delay before becoming active:
  - Shows countdown: "Review for 3s..." → "I understand, approve"
  - Deny is always active
- macOS keyboard shortcuts: ⌘D = deny (default), ⌘A = approve (not default)

#### Screen 10: Proposal History
- List view accessible from Settings or toolbar
- Each row shows:
  - Action description (truncated)
  - Tier badge (colored dot or pill)
  - Status: ✅ Approved / ❌ Denied
  - Timestamp
- Tap/click to view full proposal details
- Filter by status or tier

### Phase 5 — Permission System (macOS + iOS)

#### Screen 11: Tool Permissions
- Accessible from Settings
- Top: preset selector with 3 options as segmented control or large buttons:
  - **Safe** — "Conservative defaults for cautious use."
  - **Power User** — "Fewer confirmations, higher autonomy."
  - **Custom** — "Fine-grained control."
- Below preset selector: list of tool categories with toggles:
  - **File Access** — Read files, Write files, Delete files
  - **Shell** — Execute commands
  - **Network** — Web requests, API calls
  - **Memory** — Read memory, Write memory
- Each toggle shows current state
- Changing any toggle auto-switches preset to "Custom"
- "Reset to preset" button at bottom

### Phase 5 — Skill Marketplace (macOS + iOS)

#### Screen 12: Marketplace Browse
- macOS: grid view (3 columns). iOS: list view
- Each skill card:
  - Icon (placeholder: colored circle with first letter)
  - Skill name (bold)
  - Short description (1 line, textSecondary)
  - Trust badge: "Verified" (green) / "Local" (blue) / "Unsigned" (gray, CLI-only note)
  - "Installed" badge if installed (accent color)
  - Install / Remove button
- Top: search bar + category filter pills
- Categories: All, Productivity, Development, Communication, Data

#### Screen 13: Skill Detail
- Full screen (iOS) or sheet (macOS)
- Skill icon (large) + name + author
- Version + last updated
- Full description (multi-paragraph)
- **Capabilities section:** list of what this skill can access:
  - "Network access: api.weather.gov, wttr.in" (green checkmarks)
  - "File access: none" 
  - "Shell access: none"
- Trust tier explanation: "This skill is signed by the Fawx marketplace and has been reviewed."
- Install / Remove / Update button (full-width on iOS)

### Phase 5 — Cost Tracking (macOS + iOS)

#### Screen 14: Cost Dashboard
- Accessible from Settings or status bar tap
- **Current session:** token count + estimated cost
- **Today / This week / This month** toggle (segmented control)
- Per-provider breakdown:
  - Provider name + model name
  - Token usage (input/output)
  - Estimated cost
  - Bar chart visualization
- Total at bottom
- "Cost tracking is estimated based on published API pricing" disclaimer (textSecondary, small)

### Phase 5 — Synthesis / Custom Instructions (macOS + iOS)

#### Screen 15: Synthesis Editor
- Accessible from Settings
- Heading: "Custom Instructions"
- Subheading: "These instructions are included in every conversation." (textSecondary)
- Large text editor area (multi-line, monospace-optional)
- Character count / token estimate below editor: "342 characters · ~85 tokens"
- **Save** button (accent) + **Reset to default** button (secondary)
- Preview section (collapsed by default): "How this affects the agent" with sample system prompt preview

### Phase 5 — OpenAI PKCE OAuth (macOS + iOS)

#### Screen 16: ChatGPT Login Flow
- Part of the Add Provider flow (Screen 3) or accessible from Settings
- "Sign in with your ChatGPT subscription" large button (OpenAI branded)
- After tapping: status changes to "Waiting for authentication..." with spinner
- On success: shows connected account info:
  - "Connected as [email]"
  - "Access to: GPT-5.4, GPT-4o, ..." (model list)
  - "Disconnect" button (destructive style)
- On failure: error message with "Try again" button

### Phase 5 — Remote VPS Pairing (macOS + iOS)

#### Screen 17: Remote Server Pairing
- Accessible from Settings → "Connect to remote server"
- Distinct from local setup — different visual treatment to make clear this is remote
- Two connection methods (tabbed or toggled):
  - **Pairing Code:** "Enter the 6-character code shown on your server" + code input (6 boxes, auto-advance)
  - **Manual:** Server URL + Bearer Token inputs (like current onboarding but labeled as "Remote")
- "Test Connection" button with inline result
- Connection status indicator: 🟢 Connected / 🔴 Unreachable / 🟡 Testing
- "This server is not on your local machine" info banner (surface background, info icon)

---

## Output Format

Single HTML file with:
- Tabbed navigation across all 17 screens (grouped by phase)
- Each screen has **macOS + iOS variants side by side** (or toggled via button)
- **Dark mode + light mode toggle** in the top corner — every screen needs both modes
- Screenshot-ready at native resolution
- Exact design system values from the reference — no approximations
- Responsive enough to screenshot at different widths without breaking

## Design Tabs Structure

```
Phase 4: Setup Wizard
  [Welcome] [Tailscale] [Add Provider] [You're Ready]

Phase 4: System
  [Menu Bar] [Server Settings] [iPhone Pairing]

Phase 5: Safety
  [Proposal Prompt] [Diff Viewer] [Proposal History] [Permissions]

Phase 5: Skills
  [Marketplace] [Skill Detail]

Phase 5: Settings
  [Cost Tracking] [Custom Instructions] [ChatGPT Login] [Remote Pairing]
```

---

## Key Design Principles

1. **"The agent is the interface"** — features the agent handles through conversation don't need GUI screens. These screens are for things that genuinely need visual UI.
2. **Setup wizard is skippable, non-destructive, re-enterable** from Settings. Every screen has a way out.
3. **Every approval/permission UI makes the actual operation visible**, not the agent's description of it. The agent provides a "reason" but it's visually separated and labeled.
4. **Menu bar is the persistent presence** — the main window is optional after setup.
5. **iPhone and Mac share components where possible** but respect platform idioms (bottom sheets vs panels, navigation patterns, etc.).
6. **Tier colors are consistent everywhere:** green=safe, amber=elevated, red=sensitive. Users learn the color language once.
