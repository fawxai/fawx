# Spec: Swift Main Thread I/O Audit & Fix

**Problem:** The Fawx Swift app deadlocks when doing heavy work. Spindump shows the main thread stuck in `TH_WAIT + TH_UNINT` with `TH_SFLAG_RW_PROMOTED` — a read-write lock promotion deadlock. The main thread is doing file I/O (or CoreData/filesystem operations) directly, which blocks the UI and can deadlock when multiple threads contend on the same RW lock.

**Impact:** App hangs, macOS spindump trigger, force-quit required. Happened during parameter-golf experiment run with GPT-5.4.

**Goal:** Move all file I/O off the main thread. The UI thread should never block on disk operations.

---

## Audit Scope

Search the entire Swift codebase for these patterns on the main thread:

### 1. Direct File Operations
- `FileManager.default` calls (read, write, copy, move, delete, exists, attributes)
- `Data(contentsOf:)`, `String(contentsOfFile:)`, `String(contentsOf:)`
- `try data.write(to:)`, `try string.write(to:)`
- `JSONEncoder().encode()` → `write` chains
- `JSONDecoder().decode()` from file data chains
- Any `URL` file reads/writes

### 2. UserDefaults Heavy Writes
- `UserDefaults.standard.set()` with large data (arrays, dictionaries)
- `UserDefaults.standard.synchronize()` (deprecated but might exist)

### 3. CoreData / SwiftData Main Context
- Fetches or saves on `@MainActor` or `MainActor.run`
- `viewContext.save()` or `viewContext.fetch()` without background context

### 4. Synchronous Network on Main
- `URLSession.shared.data(from:)` without `await` on a background task
- Any blocking network call

### 5. Process/Shell Execution
- `Process()` launch and `waitUntilExit()` on main thread
- Pipe reads on main thread

---

## Fix Pattern

For every violation found, apply this pattern:

### Before (bad)
```swift
// On main thread
let data = try Data(contentsOf: fileURL)
let config = try JSONDecoder().decode(Config.self, from: data)
self.config = config
```

### After (good)
```swift
// File I/O on background, UI update on main
Task.detached(priority: .utility) {
    let data = try Data(contentsOf: fileURL)
    let config = try JSONDecoder().decode(Config.self, from: data)
    await MainActor.run {
        self.config = config
    }
}
```

### For ObservableObject / @Published properties
```swift
func loadConfig() async {
    let config = await Task.detached(priority: .utility) {
        let data = try Data(contentsOf: self.fileURL)
        return try JSONDecoder().decode(Config.self, from: data)
    }.value
    
    // Back on MainActor for @Published update
    self.config = config
}
```

### For fire-and-forget writes
```swift
func saveConfig(_ config: Config) {
    Task.detached(priority: .utility) {
        let data = try JSONEncoder().encode(config)
        try data.write(to: self.fileURL, options: .atomic)
    }
}
```

---

## RW Lock Specific Fix

The spindump showed `TH_SFLAG_RW_PROMOTED` — a read lock trying to upgrade to write. If the codebase uses `pthread_rwlock` or `os_unfair_lock` or any custom RW lock:

1. **Find all RW lock usage** — search for `NSLock`, `NSRecursiveLock`, `DispatchQueue` (barrier), `os_unfair_lock`, `pthread_rwlock`, `actor` isolation boundaries
2. **Never upgrade read → write** — always release read, then acquire write
3. **Prefer Swift actors** over manual locks where possible — they serialize access without deadlock risk
4. **If using DispatchQueue for sync**: never call `.sync` on the main queue from a background queue that the main queue might be waiting on

---

## Priority Order

Fix in this order (most likely crash causes first):

1. **Session/conversation file I/O** — reading/writing chat history, session state
2. **Experiment result I/O** — reading/writing experiment data (triggered the crash)
3. **Config file reads** — `config.toml` parsing on startup or model switch
4. **Skill/plugin loading** — WASM file reads
5. **Any RW lock patterns** — audit and refactor to actors or async

---

## Testing

1. **Stress test**: Open Fawx, start a streaming conversation with GPT-5.4 xhigh, simultaneously trigger file operations (switch model, change settings, start experiment)
2. **Thread sanitizer**: Build with Thread Sanitizer enabled (`-Xswiftc -sanitize=thread`) and run the stress test — it will catch data races and lock order violations
3. **Main thread checker**: Ensure Xcode's Main Thread Checker is enabled in the scheme diagnostics — it flags UIKit/AppKit calls from background threads (the inverse problem)
4. **Instruments Time Profiler**: Run and verify the main thread never blocks >16ms on I/O

---

## Acceptance Criteria

- [ ] No `Data(contentsOf:)` or file writes on `@MainActor` or main thread
- [ ] No RW lock upgrade patterns (read → write on same thread)
- [ ] Thread Sanitizer passes on a streaming + file I/O stress test
- [ ] App survives 10min GPT-5.4 xhigh streaming session without hang
- [ ] All file I/O wrapped in `Task.detached` or background actor
