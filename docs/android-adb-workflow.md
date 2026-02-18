# ADB Development Workflow for Citros

> Issue: [#132](https://github.com/abbudjoe/citros/issues/132)  
> Audience: developers iterating on Android app + Rust binary integration

This document defines the standard ADB loop for fast, reproducible Android development.

## Prerequisites

- Android SDK Platform Tools (`adb`) installed
- Device with Developer options and USB debugging enabled
- For physical devices over USB, host key authorized

Optional but recommended:
- Rooted test device (see `docs/android-root-magisk-setup.md`)

---

## 1) Device Discovery + Basic Health

```bash
adb devices -l
```

Common states:
- `device` → ready
- `unauthorized` → accept host prompt on phone
- no device listed → check cable/port/driver

Useful quick checks:

```bash
adb shell getprop ro.product.model
adb shell getprop ro.build.version.release
```

---

## 2) Android App Build + Install Loop

From repo root:

```bash
cd android
./gradlew :app:installDebug
```

Launch app manually, or via ADB:

```bash
adb shell monkey -p ai.citros.app -c android.intent.category.LAUNCHER 1
```

Clear app data (clean-state testing):

```bash
adb shell pm clear ai.citros.app
```

Uninstall app:

```bash
adb uninstall ai.citros.app
```

---

## 3) Rust Binary Build + Push Loop

Build (from repo root):

```bash
cargo build --target aarch64-linux-android --release -p ct-cli
```

Push binary to device:

```bash
adb push target/aarch64-linux-android/release/citros /data/local/tmp/citros
adb shell chmod 755 /data/local/tmp/citros
```

Run doctor command:

```bash
adb shell /data/local/tmp/citros doctor
```

If root shell is required:

```bash
adb shell su -c '/data/local/tmp/citros doctor'
```

---

## 4) Port Forwarding / Reverse Tunnels

Citros Android OAuth bridge defaults to host port `4318`.

For physical devices:

```bash
adb reverse tcp:4318 tcp:4318
```

List reverse mappings:

```bash
adb reverse --list
```

Remove mapping:

```bash
adb reverse --remove tcp:4318
```

For emulator, use `10.0.2.2` to reach host services from the app. (Note: Citros is primarily tested on physical devices — emulator support is experimental.)

---

## 5) Logs and Debugging

Live logcat (all logs):

```bash
adb logcat
```

Filter by app package (common pattern):

```bash
adb logcat | grep -i "ai.citros"
```

### Tag-Based Filtering (Recommended)

Filter logcat to Citros-specific tags for cleaner output:

```bash
adb logcat -s CitrosAccessibility:* ChatViewModel:* PhoneAgentApi:* OverlayService:* SqliteMemoryProvider:*
```

Common Citros tags:
- `CitrosAccessibility` — Accessibility service events, screen reads, actions
- `ChatViewModel` — Message flow, tool loop, loading state
- `PhoneAgentApi` — Tool execution, API calls, agent responses
- `OverlayService` — Overlay lifecycle, mode transitions, WindowManager
- `SqliteMemoryProvider` — Memory storage, FTS5 search, queries

Combine with grep for further filtering:

```bash
adb logcat -s PhoneAgentApi:* | grep -i "tool\|error"
```

Clear log buffer before reproducing an issue:

```bash
adb logcat -c
```

Capture bug report bundle:

```bash
adb bugreport
```

---

## 6) Useful File and Shell Operations

Open shell:

```bash
adb shell
```

Pull artifacts from device:

```bash
adb pull /data/local/tmp/citros ./artifacts/citros-device
```

Inspect process list:

```bash
adb shell ps -A | grep -i citros
```

Check socket listeners:

```bash
adb shell ss -lntp
```

---

## 7) Wireless ADB (Optional)

Useful when USB is inconvenient:

```bash
adb tcpip 5555
adb connect <device-ip>:5555
adb devices -l
```

Security note: only use wireless ADB on trusted networks. Disable when done.

---

## 8) Common Failure Modes

### `device unauthorized`
- Reconnect USB
- Revoke USB debugging authorizations on device and re-authorize

### `more than one device/emulator`
- Specify serial in commands:

```bash
adb -s <serial> shell getprop ro.product.model
```

### `Permission denied` running `/data/local/tmp/citros`
- Ensure executable bit set: `chmod 755`
- If root-only paths/capabilities are used, run via `su -c`

### `INSTALL_FAILED_VERSION_DOWNGRADE`
- Uninstall app or bump version code

### Host service unreachable from device
- Re-run `adb reverse --list`
- Confirm host service is bound to expected port

---

## 9) Recommended Inner Loop

For fastest iteration:

1. Build/install app (`./gradlew :app:installDebug`)
2. Build/push Rust binary (`cargo build ... && adb push ...`)
3. Run smoke check (`adb shell /data/local/tmp/citros doctor`)
4. Validate app behavior + collect logs (`adb logcat`)
5. Repeat

This keeps Android UI and native daemon iteration tightly synchronized.
