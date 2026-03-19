# Spec: Swift Ripcord UI + Request Capability + Phase 3 Sandbox Toggle

**For:** Codex on Mac Mini  
**Repo:** `abbudjoe/fawx` (branch from `dev`)  
**Branch name:** `feat/swift-ripcord-ui`  
**Depends on:** PRs #1471-#1476 (all merged to dev)

---

## Context

The backend now has a complete tripwire/ripcord system:
- Tripwires silently activate when the agent crosses defined boundaries
- A journal tracks all actions since the crossing
- The user can review the journal, pull the ripcord (revert), or approve (dismiss)
- 4 API endpoints are live

This spec adds the Swift UI for the ripcord system, plus a sandbox status indicator and the `request_capability` notification flow.

---

## 1. Ripcord API Endpoints

### GET /v1/ripcord/status
```json
{
  "active": true,
  "tripwire_id": "credential_read",
  "tripwire_description": "Credential file access",
  "entry_count": 7
}
```

### GET /v1/ripcord/journal
```json
{
  "entries": [
    {
      "id": 0,
      "timestamp": "2026-03-17T05:00:00Z",
      "tool_name": "write_file",
      "tool_call_id": "call_123",
      "action": {
        "type": "file_write",
        "path": "/home/user/project/src/main.rs",
        "snapshot_hash": "abc123",
        "size_bytes": 1024,
        "created": false
      },
      "reversible": true
    },
    {
      "id": 1,
      "timestamp": "2026-03-17T05:01:00Z",
      "tool_name": "shell",
      "tool_call_id": "call_456",
      "action": {
        "type": "shell_command",
        "command": "cargo build",
        "exit_code": 0
      },
      "reversible": false
    }
  ]
}
```

### POST /v1/ripcord/pull
```json
{
  "reverted": [
    { "id": 0, "tool_name": "write_file", "description": "Restored from snapshot" }
  ],
  "skipped": [
    { "id": 1, "tool_name": "shell", "reason": "Shell command side effects cannot be reverted (audit only)" }
  ],
  "total": 2
}
```

### POST /v1/ripcord/approve
```json
{ "cleared": true }
```

---

## 2. Ripcord Status Indicator

**Location:** Top of the chat view or in the toolbar/status area.

**When ripcord is inactive:** Nothing shown.

**When ripcord is active:** Show a persistent, non-intrusive banner:

```
🔔 Ripcord Active — "Credential file access" — 7 actions journaled
   [Review]  [Pull Ripcord]  [Approve]
```

- The banner polls `/v1/ripcord/status` every 5 seconds (or uses SSE if available)
- Yellow/amber color to indicate monitoring without alarm
- Tapping "Review" opens the journal panel
- Tapping "Pull Ripcord" shows a confirmation dialog then calls POST /v1/ripcord/pull
- Tapping "Approve" shows a confirmation then calls POST /v1/ripcord/approve

**On tripwire crossing (new activation):** Show a macOS notification:
```
Fawx — Tripwire Crossed
"Credential file access"
Actions are being journaled. Review when ready.
```

---

## 3. Journal Review Panel

**Trigger:** "Review" button on the ripcord banner, or a menu item.

**Layout:** Slide-out panel or sheet (same pattern as experiment monitor detail).

**Content:**
```
Ripcord Journal
Tripwire: "Credential file access"
Active since: 5:00 PM

┌─────────────────────────────────────────────────┐
│ #0  write_file — src/main.rs                    │
│     5:00 PM  ✅ Reversible  1.0 KB              │
├─────────────────────────────────────────────────┤
│ #1  shell — cargo build                         │
│     5:01 PM  ⚠️ Audit only                      │
├─────────────────────────────────────────────────┤
│ #2  write_file — src/lib.rs                     │
│     5:02 PM  ✅ Reversible  2.3 KB              │
├─────────────────────────────────────────────────┤
│ #3  git_commit — abc1234                        │
│     5:03 PM  ✅ Reversible                       │
└─────────────────────────────────────────────────┘

[Pull Ripcord — Undo All]    [Approve — Keep Changes]
```

**Entry display:**
- Reversible entries: green checkmark badge
- Audit-only entries: yellow warning badge
- Show tool name, relevant detail (path, command, commit SHA), timestamp, size where applicable

**After pulling ripcord:** Show the report:
```
Ripcord Pulled
✅ Reverted: 3 actions
⚠️ Skipped: 1 action (audit only)

Reverted:
  • write_file — src/main.rs (restored from snapshot)
  • write_file — src/lib.rs (restored from snapshot)
  • git_commit — reset to previous state

Skipped:
  • shell — cargo build (cannot undo shell commands)

[Done]
```

---

## 4. Sandbox Status (Phase 3 placeholder)

**Location:** Settings > Security section, below the capability mode toggle.

**Content:**
```
OS Sandbox
┌─────────────────────────────────────────┐
│  🔒 Not available                       │
│  OS-level enforcement requires Linux    │
│  5.13+ with Landlock support.           │
│                                         │
│  Your security is enforced at the       │
│  application level via capability mode. │
└─────────────────────────────────────────┘
```

This is a static display for now — Phase 3 backend isn't built yet. When it is, this becomes:
```
OS Sandbox  [Enabled ✅]
  Filesystem: Landlock ✅
  Syscalls: seccomp ✅
  Network: nftables ✅
```

Just build the UI frame with the "not available" state. The dynamic state will come later.

---

## 5. Data Models

### RipcordStatus
```swift
struct RipcordStatusResponse: Codable, Sendable {
    let active: Bool
    let tripwireId: String?
    let tripwireDescription: String?
    let entryCount: Int

    enum CodingKeys: String, CodingKey {
        case active
        case tripwireId = "tripwire_id"
        case tripwireDescription = "tripwire_description"
        case entryCount = "entry_count"
    }
}
```

### JournalEntry
```swift
struct JournalEntry: Codable, Sendable, Identifiable {
    let id: Int
    let timestamp: String
    let toolName: String
    let toolCallId: String
    let action: JournalAction
    let reversible: Bool

    enum CodingKeys: String, CodingKey {
        case id, timestamp, action, reversible
        case toolName = "tool_name"
        case toolCallId = "tool_call_id"
    }
}

// JournalAction is a tagged enum — decode based on "type" field
struct JournalAction: Codable, Sendable {
    let type: String
    // Additional fields vary by type — use a flexible decoder or
    // store the raw JSON and extract display info
}
```

### RipcordReport
```swift
struct RipcordReport: Codable, Sendable {
    let reverted: [RevertedEntry]
    let skipped: [SkippedEntry]
    let total: Int
}

struct RevertedEntry: Codable, Sendable, Identifiable {
    let id: Int
    let toolName: String
    let description: String

    enum CodingKeys: String, CodingKey {
        case id, description
        case toolName = "tool_name"
    }
}

struct SkippedEntry: Codable, Sendable, Identifiable {
    let id: Int
    let toolName: String
    let reason: String

    enum CodingKeys: String, CodingKey {
        case id, reason
        case toolName = "tool_name"
    }
}
```

---

## 6. API Client Methods

Add to `FawxClient.swift`:
```swift
func ripcordStatus() async throws -> RipcordStatusResponse
func ripcordJournal() async throws -> RipcordJournalResponse  // { entries: [JournalEntry] }
func pullRipcord() async throws -> RipcordReport
func approveRipcord() async throws  // returns { cleared: true }
```

---

## 7. Files to Create/Modify

### New files:
- `app/Fawx/Models/Ripcord.swift` — data models
- `app/Fawx/Views/Ripcord/RipcordBanner.swift` — status banner for chat view
- `app/Fawx/Views/Ripcord/RipcordJournalPanel.swift` — journal review sheet
- `app/Fawx/Views/Ripcord/RipcordReportView.swift` — post-pull report
- `app/Fawx/Views/Settings/SandboxStatusCard.swift` — Phase 3 placeholder

### Modified files:
- `app/Fawx/Networking/FawxClient.swift` — add 4 API methods
- `app/Fawx/ViewModels/AppState.swift` — add ripcord polling, status tracking
- `app/Fawx/Views/Chat/ChatView.swift` — add ripcord banner
- `app/Fawx/Views/Settings/PermissionsSettingsPanel.swift` — add sandbox status card

---

## 8. Build & Test

```bash
cd ~/fawx
git fetch origin && git checkout -b feat/swift-ripcord-ui origin/dev
# Make changes
xcodebuild -scheme Fawx -destination 'platform=macOS' build
```

## What NOT to Change
- Do not modify Rust/engine code
- Do not implement the `request_capability` tool notification yet — that's a follow-up once the tool exists in the engine
- Do not implement real sandbox status checking — just the static placeholder
