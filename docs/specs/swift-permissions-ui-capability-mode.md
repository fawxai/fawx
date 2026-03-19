# Spec: Swift Permissions UI — Capability Mode Update

**For:** Codex on Mac Mini  
**Repo:** `abbudjoe/fawx` (branch from `dev`)  
**Branch name:** `feat/swift-capability-mode-ui`

---

## Context

The backend permissions API changed (PR #1471, merged to dev). The security model now has two modes:

- **Capability mode** (default): restricted actions are silently denied. No prompts. No modals.
- **Prompt mode** (opt-in): restricted actions trigger interactive approval prompts (legacy behavior).

The API now returns a `mode` field in the permissions response and accepts it in PATCH requests.

## API Changes

### GET /v1/permissions
```json
{
  "preset": "power",
  "mode": "capability",       // NEW — "capability" or "prompt"
  "permissions": [
    { "action": "shell", "level": "denied", "title": "Shell Commands" },
    { "action": "read_any", "level": "allow", "title": "Read Files" }
  ],
  "available_presets": ["power", "cautious", "experimental", "custom"]
}
```

When `mode` is `"capability"`, restricted actions show `level: "denied"`.  
When `mode` is `"prompt"`, restricted actions show `level: "ask"`.

### PATCH /v1/permissions
```json
{
  "mode": "prompt"    // Can update mode independently
}
```

## Required UI Changes

### 1. Permissions Settings Screen

**Add a mode toggle** at the top of the permissions screen, above the preset selector:

```
Security Mode
┌─────────────────────────────────────────┐
│  🛡️ Capability (Recommended)           │
│  Actions are allowed or silently denied │
│  based on your preset. No interruptions.│
│                                         │
│  🔔 Interactive                         │
│  Restricted actions pause and ask for   │
│  your approval before proceeding.       │
└─────────────────────────────────────────┘
```

- Use a segmented control or radio group
- "Capability" maps to API value `"capability"`
- "Interactive" maps to API value `"prompt"` (user-friendly label, not "prompt")
- Changing mode sends `PATCH /v1/permissions { "mode": "capability" }` or `{ "mode": "prompt" }`
- Default selection comes from GET response `mode` field

### 2. Permission Entries Display

**Update the level badge colors:**
- `"allow"` → green badge, label "Allowed"
- `"denied"` → red badge, label "Denied" (was "Requires Approval" in old UI)
- `"ask"` → yellow/orange badge, label "Requires Approval" (only visible in prompt mode)
- `"deny"` → gray badge, label "Blocked"

### 3. Remove Permission Approval Modal (Capability Mode)

When `mode` is `"capability"`:
- **Do NOT show the permission approval modal.** It will never fire — the server doesn't send `permission_prompt` SSE events.
- The SSE listener for `permission_prompt` events should still exist (for prompt mode) but the UI should not show the modal queue or approval UI elements.

When `mode` is `"prompt"`:
- Permission approval modal works as before (if it was working — known bug with dismissal timing, but that's a separate issue).

### 4. Preset Selector

Add the new preset aliases to the selector display (optional, nice-to-have):
- "Standard" (= Power preset)
- "Restricted" (= Cautious preset)  
- "Open" (= Experimental preset)

The API still uses the original names internally.

## Files to Modify

Based on the Swift project structure on the Mac Mini:
- `app/Fawx/Views/Settings/PermissionsView.swift` — main permissions screen
- `app/Fawx/Models/Permissions.swift` or equivalent — data models for API response
- `app/Fawx/Networking/FawxAPI.swift` or equivalent — API client
- `app/Fawx/Views/Chat/PermissionPromptView.swift` — conditional display based on mode

## Build & Test

```bash
cd ~/fawx
git fetch origin && git checkout -b feat/swift-capability-mode-ui origin/dev
# Make changes
xcodebuild -scheme Fawx -destination 'platform=macOS' build
```

## What NOT to Change
- Do not modify the server/engine code (Rust). Only Swift.
- Do not remove the permission prompt SSE handling — just gate the UI on mode.
- Do not change the preset content (which actions are allowed/denied).
