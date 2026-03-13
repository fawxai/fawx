# Fawx Mockup Revisions — Codex Implementation Feedback

**Context:** You previously generated `docs/design/fawx-mockups.html` (12 screens, dark + light). Codex reviewed the screenshots for implementation readiness and flagged 3 gaps. This prompt addresses all 3.

**Existing reference files (attached or pasted alongside):**
1. The current `docs/design/fawx-mockups.html` — your prior output
2. `docs/specs/swift-app-spec.md` — the approved spec (source of truth)

**Use the exact same design system** (colors, typography, spacing) from the original prompt. All CSS variables, the dark/light toggle, and the tab navigation structure should carry over.

---

## Revision 1: iOS Model Name Truncation Rule

**Problem:** On Screen 8 (iOS Sessions) and Screen 9 (iOS Chat), the status strip shows the full model name like `claude-sonnet-4-6`. On a 390px-wide iPhone, this can wrap onto two lines, breaking the layout. Codex needs an explicit rule to implement in SwiftUI.

**Fix:** Apply these truncation rules to ALL iOS model name displays:

| Full Model Name | iOS Abbreviation |
|----------------|-----------------|
| `claude-sonnet-4-6` | `sonnet-4-6` |
| `claude-opus-4-6` | `opus-4-6` |
| `gpt-5.4` | `gpt-5.4` (already short) |
| Any model > 15 chars | Drop provider prefix before `/` or first `-`, keep the rest. If still > 15 chars, truncate with `…` |

**What to change:**
- **Screen 8** — In the status row (the 4-segment bar with Connected · Power User · model · ctx), show `sonnet-4-6` instead of `claude-sonnet-4-6`. Make the segments tight enough that they all fit on one line at 390px with no wrapping.
- **Screen 9** — In the navigation bar status header, same abbreviation. Show: `Connected · Power User · sonnet-4-6 · 62% ctx` all on one line.
- **Input bar model badge** (Screen 9 bottom) — already shows `sonnet-4-6`, which is correct. Keep it.

Add a small annotation callout near the status strip: *"Model names abbreviated for mobile — drop provider prefix (e.g., claude-sonnet-4-6 → sonnet-4-6)"*

---

## Revision 2: Split Composite Boards into Individual Full-Resolution Screens

**Problem:** Screens 7 (Settings), 10 (Empty States), and 12 (Error States) are composite boards showing 2-4 states in a grid layout. This is great for coverage overview but makes it hard for engineers to measure exact spacing from screenshots. Codex needs pixel-accurate individual screens.

**Fix:** Replace the composites with individual full-resolution screens. Keep the composites as an overview tab, but add dedicated tabs for each sub-state.

### Screen 7 → Split into 7a, 7b, 7c

**7a: Settings — Connection** (full-resolution macOS Settings window, ~600×500)
- The Connection tab as currently shown, rendered alone at full size
- Server URL field, Bearer Token (masked), connection status, Test Connection button

**7b: Settings — Model & Thinking** (full-resolution)
- Server Model dropdown (showing `claude-sonnet-4-6`)
- Server Thinking Level dropdown (showing `high`)
- Warning note: "These settings apply to all sessions on the server."

**7c: Settings — Appearance** (full-resolution)
- Whatever appearance options exist (theme picker, font size, etc.)
- If appearance is minimal in V1, show what we have — even if it's just a "Theme: System / Dark / Light" segmented control

### Screen 10 → Split into 10a, 10b, 10c, 10d

**10a: Empty State — No Sessions** (full macOS window with sidebar + content)
- Sidebar empty (just the + New Session button)
- Content: centered empty state with icon + "No conversations yet" + "Start a new session to begin chatting with Fawx" + accent "New Session" button

**10b: Empty State — No Search Results** (full macOS window)
- Sidebar with search field active, showing "No results for 'xyz'" message
- Content area showing the selected session or empty

**10c: Empty State — Load Failed** (full macOS window)
- Content area: centered error state with ⚠️ icon + "Couldn't load sessions" + "Check your connection and try again" + "Retry" button (bordered)

**10d: Empty State — Skills Empty** (full macOS window)
- Skills page with no skills loaded
- Centered: 🧩 icon + "No skills loaded" + "Skills are loaded on the Fawx server. Check your server configuration." (textSecondary)

### Screen 12 → Split into 12a, 12b, 12c, 12d

**12a: Error State — Reconnecting** (full macOS window)
- Status bar: yellow/warning dot + "Reconnecting..." (warning color)
- Content area functional but with a subtle yellow top banner: "Connection lost. Reconnecting..."
- Input bar: disabled/muted, placeholder changes to "Reconnecting..."

**12b: Error State — Disconnected** (full macOS window)
- Status bar: red dot + "Disconnected" (error color)
- Content area: chat visible but a red top banner: "Unable to connect to server"
- Input bar: disabled, "Server unreachable" placeholder

**12c: Error State — Interrupted Response** (full macOS window, chat view)
- Chat with a message that was interrupted mid-stream
- Assistant message ends abruptly with: italic "Response interrupted" label below the partial text (textSecondary)
- No blinking cursor — the stream is done

**12d: Error State — Rate Limited** (full macOS window, chat view)
- Chat with a rate limit error card after user message
- Card (warning/border): ⚠️ "Rate limited — please wait 30 seconds before sending another message"
- Input bar: temporarily disabled with countdown or just muted

---

## Revision 3: iOS Settings Detail View

**Problem:** Screen 11 shows the iOS Settings top-level list (5 sections with chevrons), but doesn't show what happens when you tap into a section. Engineers need a reference for the drill-in layout.

**Fix:** Add Screen 11b — iOS Settings detail view for "Model & Thinking".

**Screen 11b: iOS Settings — Model & Thinking (Detail)**

**iPhone frame (390×844):**

**Navigation bar:**
- Back chevron (‹) + "Model & Thinking" title (17px, semibold)

**Content (grouped list style, standard iOS form):**

**Section: Model**
- Grouped card (surface background, cornerRadius)
- Row: "Server Model" label + current value `sonnet-4-6` (textSecondary, right-aligned)
- Tapping opens a picker/action sheet — show the picker expanded:
  - ✓ `sonnet-4-6` (accent color checkmark)
  - `gpt-5.4`
  - `opus-4-6`

**Section: Thinking**
- Grouped card
- Row: "Thinking Level" label + current value `high` (textSecondary, right-aligned)
- Below: segmented control or picker showing: Off | Low | Adaptive | High | Extra High
  - "High" segment selected (accent background, white text)

**Footer note** (below sections, textSecondary, 13px):
- "Changes apply to all sessions on the server."

**Tab bar:** Same as Screen 11 (Settings tab selected)

---

## Output Format

Update the existing HTML file structure:
- Keep the existing screen tabs (1-12)
- Add new tabs for the split screens: 7a, 7b, 7c, 10a, 10b, 10c, 10d, 11b, 12a, 12b, 12c, 12d
- The dark/light toggle applies to all screens
- Each new screen must be independently screenshot-able at full resolution
- Total screens after revision: 12 original + 12 new individual exports = ~24 tabs

Alternatively, if that's too many tabs: replace the composite tabs (7, 10, 12) with their individual breakouts, and keep the rest. That would be: 9 originals + replace 3 composites with 11 individual screens + add 11b = ~21 tabs.

**Priority order if you need to limit scope:**
1. Revision 1 (iOS truncation) — smallest change, highest impact
2. Revision 3 (Screen 11b) — one new screen, removes ambiguity
3. Revision 2 (split composites) — most work, but most value for spacing accuracy
