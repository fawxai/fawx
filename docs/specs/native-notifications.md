# Native Notifications

## Problem

When Fawx is running a long task (experiments, multi-step tool use, code generation), the user has no way to know it's done without watching the app. This is a basic UX gap that every native app solves with OS notifications.

## Design Principles

**The agent decides when to notify.** Notifications are a communication channel. The agent knows what's worth interrupting the user for and what to say. A score improvement deserves a ping; a trivial echo does not. This is AX-first: the agent controls its own voice.

**Automatic fallback for safety.** If the agent completes a multi-tool cycle without calling `notify`, and the app isn't frontmost, fire a generic "Task complete" notification after a short grace period. Safety net without undermining agent control.

## Solution

### 1. `notify` builtin tool

Add a new builtin tool the agent can call:

```json
{
  "name": "notify",
  "description": "Send a native OS notification to the user. Use when completing a task the user is waiting for, reporting important results, or when the app is not in focus. Do not use for trivial acknowledgements.",
  "parameters": {
    "type": "object",
    "properties": {
      "title": {
        "type": "string",
        "description": "Short notification title (1-2 words or phrase)"
      },
      "body": {
        "type": "string",
        "description": "Notification body with details"
      }
    },
    "required": ["body"]
  }
}
```

**Defaults:**
- `title` defaults to `"Fawx"` if omitted
- Notification sound: default system sound
- No custom actions (clicking opens the app to the active session)

### 2. Rust-side: NotifySkill

New file: `engine/crates/fx-loadable/src/notify_skill.rs`

```rust
#[derive(Debug)]
pub struct NotifySkill {
    sender: Arc<dyn NotificationSender>,
}

#[async_trait]
pub trait NotificationSender: Send + Sync + std::fmt::Debug {
    async fn send(&self, title: &str, body: &str) -> Result<(), String>;
}
```

The skill:
- Implements `Skill` trait
- Exposes one tool: `notify`
- Parses `title` (optional) and `body` (required) from JSON args
- Calls `NotificationSender::send()`
- Returns `"Notification sent"` on success

The `NotificationSender` trait is the bridge to the platform. The Rust side doesn't know about macOS or iOS APIs; it calls through the trait.

### 3. Platform bridge

**macOS/iOS (Swift side):**

New file: `app/Fawx/Services/NotificationService.swift`

```swift
import UserNotifications

@MainActor
final class NotificationService {
    static let shared = NotificationService()

    func requestPermission() async -> Bool {
        let center = UNUserNotificationCenter.current()
        do {
            return try await center.requestAuthorization(options: [.alert, .sound, .badge])
        } catch {
            return false
        }
    }

    func send(title: String, body: String) async {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.sound = .default

        let request = UNNotificationRequest(
            identifier: UUID().uuidString,
            content: content,
            trigger: nil  // deliver immediately
        )

        try? await UNUserNotificationCenter.current().add(request)
    }
}
```

**Wire into HTTP API:**

The skill runs in the Rust server. The Swift app connects via HTTP. Two options for the bridge:

**Option A (recommended): Server-Sent Events (SSE)**
The app already subscribes to SSE for streaming. Add a `notification` event type:
```json
{"event": "notification", "data": {"title": "Fawx", "body": "Task complete"}}
```
The Swift SSE handler receives this and calls `NotificationService.send()`.

**Option B: HTTP endpoint**
`POST /v1/notify` — the skill calls this, the app polls or the server pushes. More complex, less real-time.

Option A is better because the SSE channel already exists.

### 4. Automatic fallback

In the agentic loop completion path (wherever `CycleResult` is finalized):

```rust
// After cycle completes, if notify tool was NOT called during this cycle:
if !cycle_context.notify_was_called && cycle_result.iterations > 1 {
    // Send generic notification via SSE
    sse_sender.send(SseEvent::Notification {
        title: "Fawx".into(),
        body: format!("Task complete ({} steps)", cycle_result.iterations),
    });
}
```

**Conditions for automatic fallback:**
- `notify` was NOT called during this cycle
- Cycle had >1 iteration (skip single-turn responses)
- App is not frontmost (the Swift side checks `NSApp.isActive` before showing)

The Swift side suppresses notifications when the app is active:

```swift
func send(title: String, body: String) async {
    #if os(macOS)
    guard !NSApp.isActive else { return }
    #else
    guard UIApplication.shared.applicationState != .active else { return }
    #endif
    // ... send notification
}
```

### 5. Permission request

Call `NotificationService.shared.requestPermission()` during the setup wizard's Ready step, or on first app launch after setup. One-time prompt. If denied, notifications silently no-op (no error to the agent).

## Files to create/change

| File | Change |
|------|--------|
| `engine/crates/fx-loadable/src/notify_skill.rs` | New: `NotifySkill` + `NotificationSender` trait |
| `engine/crates/fx-loadable/src/lib.rs` | Add `pub mod notify_skill` |
| `engine/crates/fx-cli/src/startup.rs` | Register `NotifySkill` in skill registry |
| `engine/crates/fx-api/src/handlers/message.rs` | SSE `notification` event type |
| `engine/crates/fx-api/src/types.rs` | `SseEvent::Notification` variant |
| `app/Fawx/Services/NotificationService.swift` | New: UNUserNotificationCenter wrapper |
| `app/Fawx/Services/StreamingService.swift` | Handle `notification` SSE event |
| `app/Fawx/Views/Shared/SetupWizard/ReadyStep.swift` | Request notification permission |
| `engine/crates/fx-kernel/src/loop_engine.rs` | Automatic fallback after cycle completion |

## Tests required

1. **NotifySkill:** parses args, calls sender, returns success string
2. **NotifySkill:** missing `body` returns error
3. **NotifySkill:** default title when omitted
4. **Automatic fallback:** fires when notify not called + iterations > 1
5. **Automatic fallback:** does NOT fire when notify was called
6. **Automatic fallback:** does NOT fire for single-iteration cycles
7. **SSE event:** `notification` event serializes correctly

## Out of scope

- Notification categories/actions (reply, snooze)
- Badge count management
- Notification grouping/threading
- Custom sounds
- Rich notifications (images, progress)
- Notification history in-app

These are all future enhancements once the base channel works.
