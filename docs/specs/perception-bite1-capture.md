# Perception Engine — Bite 1: Background Capture

**Status:** Implementation-ready spec
**Date:** 2026-03-26
**Parent doc:** `docs/vision/perception-engine.md`
**Repo:** Standalone Swift package, `fawx-capture/` in fawx repo root (not in engine/)

---

## Goal

Collect raw (frame, input_action) pairs from normal Mac usage. No inference, no model. Understand the dataset shape and volume before building anything else.

---

## Scope

macOS-only data collection sidecar. Nothing ships to users. No Fawx engine changes. Output is a local dataset.

---

## Architecture: 4 Swift files

```
fawx-capture/
  Package.swift
  Sources/
    main.swift              — entry point, menu bar app, session lifecycle
    ScreenCapture.swift     — ScreenCaptureKit frame capture
    InputMonitor.swift      — CGEventTap for mouse/keyboard/scroll
    SessionWriter.swift     — disk I/O, JPEG compression, JSONL events
```

### Why Swift
- ScreenCaptureKit and CGEventTap are native macOS APIs
- No FFI bridging, no external dependencies
- Menu bar app via NSStatusItem is trivial in Swift
- Swift Package Manager for build (no Xcode project needed)

### Why NOT Rust
- objc2 bridging for ScreenCaptureKit is painful and poorly documented
- CGEventTap requires CoreFoundation run loop integration
- This is a data collection tool, not an engine component
- When fawx-eyes (Bite 3) needs to integrate with the engine, we'll evaluate Rust vs Swift IPC

---

## Screen Capture: ScreenCaptureKit (not CGWindowListCreateImage)

`CGWindowListCreateImage` is legacy. ScreenCaptureKit (macOS 12.3+) is the modern API with better performance, privacy controls, and frame streaming.

### Key APIs:
- `SCShareableContent.current()` — enumerate displays/windows/apps
- `SCContentFilter(display:excludingApplications:exceptingWindows:)` — filter what to capture
- `SCStreamConfiguration` — set resolution, frame rate, pixel format
- `SCStream` + `SCStreamOutput` — receive `CMSampleBuffer` frames at configured rate

### Configuration:
```swift
let config = SCStreamConfiguration()
config.width = 1920  // or display native / 2 for retina
config.height = 1080
config.minimumFrameInterval = CMTime(value: 1, timescale: 5)  // 5 fps
config.pixelFormat = kCVPixelFormatType_32BGRA
config.showsCursor = true
config.capturesAudio = false
```

### TCC Permission:
- ScreenCaptureKit triggers the **Screen Recording** permission prompt automatically on first use
- If denied, `SCShareableContent.current()` returns empty content
- Handle gracefully: check if content is empty, show menu bar warning icon, log instructions

---

## Input Monitoring: CGEventTap

### Key APIs:
- `CGEvent.tapCreate(tap:place:options:eventsOfInterest:callback:userInfo:)` — passive event tap
- Event types: `.leftMouseDown`, `.rightMouseDown`, `.keyDown`, `.scrollWheel`, `.mouseMoved`

### Secure input suppression:
When a password field or secure text entry is focused, macOS enables secure event input. **All keyboard event recording must be suppressed during secure input mode** to avoid leaking password length or timing.

```swift
// Check before recording any key event:
if SecureEventInputEnabled() {
    // Emit a boundary event instead of key data
    // {"type":"secure_input_start"} / {"type":"secure_input_end"}
    return
}
```

Poll `SecureEventInputEnabled()` in the CGEventTap callback. When it transitions true, emit `{"type":"secure_input_start"}`. When it transitions false, emit `{"type":"secure_input_end"}`. No key events between these boundaries.

### Privacy-safe recording:
```swift
// Keyboard: record ONLY modifier flags + key category, never actual characters
// ONLY when SecureEventInputEnabled() == false
struct KeyEvent {
    let timestamp_ms: UInt64
    let modifiers: [String]       // ["cmd", "shift"] etc.
    let category: String          // "letter", "number", "special", "function", "return", "space"
    // NO keyCode, NO characters field
}

// Mouse: record position + button + click count
struct MouseEvent {
    let timestamp_ms: UInt64
    let x: Double
    let y: Double
    let button: String            // "left", "right", "other"
    let clickCount: Int
}

// Scroll: record direction + delta
struct ScrollEvent {
    let timestamp_ms: UInt64
    let deltaX: Double
    let deltaY: Double
}
```

### TCC Permission:
- CGEventTap requires **Input Monitoring** permission (System Settings > Privacy > Input Monitoring)
- Passive taps (`.listenOnly`) don't require Accessibility, only Input Monitoring
- If not granted, `CGEvent.tapCreate` returns `nil`
- Handle gracefully: check for nil, show menu bar warning, log instructions

---

## Active App Context: NSWorkspace

```swift
// Poll frontmost app on each frame capture
let app = NSWorkspace.shared.frontmostApplication
let bundleId = app?.bundleIdentifier ?? "unknown"
let appName = app?.localizedName ?? "unknown"

// Window title via Accessibility API (optional, requires Accessibility permission)
// For Bite 1, skip window title — just bundle ID is sufficient
// Window title adds Accessibility TCC requirement which is heavy
```

Decision: **Bite 1 records bundle ID only, not window title.** This avoids the Accessibility permission entirely. We need Screen Recording + Input Monitoring, that's it.

---

## Session Writer

### Output format:
```
~/.fawx/capture/
  YYYY-MM-DD/
    session_<unix_timestamp>/
      frames/
        000001_1711234567890.jpg    (JPEG 60%, ~50-80KB)
        000002_1711234568090.jpg
        ...
      events.jsonl                  (one JSON object per line)
      metadata.json                 (session info)
```

### events.jsonl format:
```json
{"ts":1711234567890,"type":"frame","id":1,"app":"com.apple.mail"}
{"ts":1711234567950,"type":"click","x":340,"y":520,"btn":"left","n":1,"app":"com.apple.mail"}
{"ts":1711234568100,"type":"key","mods":["cmd"],"cat":"letter","app":"com.apple.Safari"}
{"ts":1711234568200,"type":"scroll","dx":0,"dy":-3.5,"app":"com.apple.Safari"}
{"ts":1711234568290,"type":"frame","id":2,"app":"com.apple.Safari"}
```

App-switch events for explicit transition tracking:
```json
{"ts":1711234568050,"type":"app_switch","from":"com.apple.mail","to":"com.apple.Safari"}
```

Compact keys to minimize file size. Frame events interleaved with input events, all in one timeline.

### metadata.json:
```json
{
  "version": 1,
  "started_at": "2026-03-26T20:15:00Z",
  "ended_at": "2026-03-26T22:30:00Z",
  "fps_target": 5,
  "display": {"width": 3456, "height": 2234, "scale": 2},
  "capture_resolution": {"width": 1728, "height": 1117},
  "jpeg_quality": 0.6,
  "frame_count": 45000,
  "event_count": 12340,
  "excluded_apps": ["com.1password.1password", "com.agilebits.onepassword7"]
}
```

### Storage management:
- JPEG 60% at half-retina resolution: ~50-80KB per frame
- At 5 fps: ~250-400KB/sec = ~0.9-1.4GB/hour
- 4-hour session: ~3.5-5.5GB
- Write frames async on a background queue to avoid blocking capture
- Flush events.jsonl every 100 events (not every event)

### Disk space safety:
- **Pre-start check:** Before starting a session, verify at least `min_free_disk_gb` (default: 10GB) of free space. Refuse to start with a menu bar warning if below threshold.
- **Max session size:** Configurable `max_session_gb` (default: 8GB) in config.json. Auto-pause capture when session crosses this limit. Show menu bar warning.
- **Max session duration:** Configurable `max_session_hours` (default: 8) in config.json. Auto-pause when exceeded. Safety net against forgotten sessions.
- **Runtime monitoring:** Check free disk space every 60 seconds during capture. If free space drops below `min_free_disk_gb`, auto-pause with menu bar warning.
- **ENOSPC handling:** If a frame write fails due to disk full, catch the error, auto-pause capture, show red menu bar icon with "Disk full" message. Do not crash. Events written up to that point are preserved (append-only JSONL + individual frame files).

---

## Menu Bar App (main.swift)

```
┌──────────────┐
│  📸 fawx     │  ← NSStatusItem in menu bar
├──────────────┤
│ ● Recording  │  ← status indicator (green dot = recording, yellow = paused, red = error)
│ ──────────── │
│ Pause        │  ← toggle pause/resume
│ ──────────── │
│ Session: 45m │  ← current session duration
│ Frames: 13k  │  ← frame count
│ Disk: 1.2GB  │  ← session disk usage
│ ──────────── │
│ Quit         │
└──────────────┘
```

### Lifecycle:
1. App launches → check TCC permissions → start session
2. If permissions missing → show warning icon with instructions
3. On app exclusion: stop frame capture when excluded app is frontmost, resume when switching away. **Note:** overlays/popups from excluded apps (e.g., 1Password autofill) appearing over non-excluded apps are an accepted risk in Bite 1. Detecting overlay windows requires the Accessibility API (extra TCC permission). Revisit in a later bite if this proves problematic.
4. On pause: stop both capture and input monitoring
5. On quit: write metadata.json, close session cleanly
6. On crash/force-quit: events.jsonl is append-only (already flushed), frames are individual files. Metadata will be missing but data is recoverable.

---

## Excluded Apps (configurable)

Default exclusion list in `~/.fawx/capture/config.json`:
```json
{
  "fps": 5,
  "jpeg_quality": 0.6,
  "capture_scale": 0.5,
  "min_free_disk_gb": 10,
  "max_session_gb": 8,
  "max_session_hours": 8,
  "excluded_apps": [
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.apple.keychainaccess",
    "com.bitwarden.desktop"
  ]
}
```

User can edit this file. App reads it on launch. Config reload via:
- **"Reload Config" menu item** in the menu bar dropdown
- **FSEvents file watcher** on `config.json` for automatic reload on save
- SIGHUP also supported as a fallback for scripted reload

---

## Build & Run

```bash
cd fawx-capture
swift build -c release

# Code sign (required for TCC Screen Recording prompt on macOS 14+)
# Ad-hoc signing is sufficient for local development:
codesign --sign - --force .build/release/fawx-capture

# For reliable TCC behavior, use a Developer ID or local signing identity:
# codesign --sign "Developer ID Application: ..." .build/release/fawx-capture

# Run
.build/release/fawx-capture

# Or during development (ad-hoc signed automatically by swift run)
swift run
```

**Important:** Unsigned Swift binaries may silently fail to receive Screen Recording TCC permission on macOS 14+. `SCShareableContent.current()` returns empty content instead of prompting the user. Always ad-hoc sign at minimum.

No Xcode project. Pure Swift Package Manager. The `Package.swift` declares macOS 13.0+ deployment target for ScreenCaptureKit availability.

---

## Success Criteria

After one work session (2-4 hours):
- [ ] Frames captured at 5fps without crashes or memory leaks
- [ ] Input events recorded with timestamps (within 10ms of actual event time)
- [ ] Frame-event interleaving is correct in events.jsonl
- [ ] Excluded apps cause capture to pause/skip
- [ ] Disk usage matches estimates (~1-1.5GB/hour)
- [ ] Menu bar shows live stats
- [ ] Pause/resume works instantly

After 1 week:
- [ ] Dataset size and shape understood
- [ ] Frame quality sufficient for UI element detection at half-retina
- [ ] Decision: 5fps constant vs event-driven capture
- [ ] No permission prompts after initial grant
- [ ] No performance impact on normal Mac usage (< 5% CPU)

---

## What We Learn

- Data volume per hour (guides storage planning)
- Whether 5fps is enough or events cluster too far from frames
- Which apps dominate usage (focus areas for planner training)
- Frame quality baseline before Florence-2

---

## Open Questions (to resolve during implementation)

1. **Half retina or quarter retina?** Half retina (1728x1117 on 3456x2234 display) gives clear text. Quarter would halve storage but might lose small UI elements. Start with half, evaluate.

2. **Frame numbering reset per session or global?** Per session (simpler, no state between sessions).

3. **Multi-display?** Bite 1: capture primary display only. Multi-display is a Bite 3 concern.

4. **What happens when Mac sleeps?** ScreenCaptureKit stream pauses automatically. Resume on wake. Log the gap in events.jsonl as a `{"type":"sleep"}` / `{"type":"wake"}` event.

5. **What happens on display changes?** (external monitor connect/disconnect, resolution change) Listen for `NSApplication.didChangeScreenParametersNotification`. On change, stop the current SCStream and start a new one with updated dimensions. Log `{"type":"display_change","width":...,"height":...}` in events.jsonl. Frames before and after may have different dimensions; metadata.json records the initial resolution, and display_change events mark transitions.

---

## Next: Bite 2

Run Florence-2 on a sample of captured frames and measure UI element detection quality on real screenshots.
