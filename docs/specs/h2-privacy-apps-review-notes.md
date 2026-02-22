# H2.6 Privacy Apps Review Notes

This file stores implementation/review history that was previously embedded in `docs/specs/h2-privacy-apps.md`.
The normative behavior contract remains in the spec; iterative review notes live here.

## Review Round 1 — Fixes

### Fix 1: Foreground Detection Reliability

`findAppWindowRoot()` can return stale/wrong packages during transitions. Mitigations:

1. **Double-check via `rootInActiveWindow`**: After `findAppWindowRoot()`, also check `svc.rootInActiveWindow?.packageName`. If they disagree, use `rootInActiveWindow` (more current).
2. **Fail-secure for privacy**: If either source returns a privacy-listed package, treat as private. This is the conservative direction; false positives are better than false negatives.
3. **Document accepted risk**: During rapid app switching (<300ms), detection may see the transitioning-away app. The next read should self-correct.

```kotlin
internal fun detectForegroundPackage(svc: AccessibilityService): String? {
    val windowPkg = findAppWindowRoot(svc)?.packageName?.toString()
    val rootPkg = svc.rootInActiveWindow?.packageName?.toString()
    return rootPkg ?: windowPkg
}

fun isPrivacyBlocked(svc: AccessibilityService): Boolean {
    val windowPkg = findAppWindowRoot(svc)?.packageName?.toString()
    val rootPkg = svc.rootInActiveWindow?.packageName?.toString()
    return (windowPkg != null && privacyList?.isPrivate(windowPkg) == true) ||
        (rootPkg != null && privacyList?.isPrivate(rootPkg) == true)
}
```

### Fix 2: Screenshot Privacy Result Type

Use a sealed result to distinguish privacy blocks from failures:

```kotlin
sealed class ScreenshotResult {
    data class Success(val base64Png: String) : ScreenshotResult()
    data object PrivacyBlocked : ScreenshotResult()
    data class Failed(val reason: String? = null) : ScreenshotResult()
}

suspend fun takeScreenshot(): ScreenshotResult {
    val svc = getService() ?: return ScreenshotResult.Failed("Accessibility service unavailable")
    if (isPrivacyBlocked(svc)) return ScreenshotResult.PrivacyBlocked
    // ... capture logic ...
}
```

### Fix 3: Notification Content Handling

Notifications from privacy-listed apps can contain sensitive data. Mitigations:

1. **Fail secure in System UI reads**: Block `read_screen` output (`privacyMode=true`) when `com.android.systemui` is foreground and privacy list is non-empty.
2. **Redact surfaced package identifiers**: Return `private_app` on privacy-mode surfaces.
3. **Document limitation**: Conservative behavior can block benign quick-settings reads while privacy mode is active.

### Fix 4: Privacy List Persistence

Status: **Deferred from PR #688**.

Backlog link: [`docs/backlog/privacy-list-persistence.md`](../backlog/privacy-list-persistence.md)

Rationale:

1. Current manifests run with `android:allowBackup="false"` for security hardening.
2. First-run prompting requires product/UX decisions and a persisted "shown once" contract.
3. This PR scope is runtime privacy enforcement; persistence policy is tracked separately.

### Fix 5: Split-Screen Gap

In split-screen mode, non-focused private app content can still be visible to `getScreenContent()`.
Conservative fix: if **any visible window** belongs to a private app, block the entire screen read.

### Fix 6: Privacy Block Logging

Add structured logging/metrics without sensitive identifiers:

- Use `privacy_block source=<source> blocked=true`
- Keep counters in-memory only
- Do not persist raw package names

### Fix 7: click_element Explicit Handling

Block `clickElement()` / `longPressElement()` explicitly during privacy mode, rather than relying on empty element lists.

### Updated Test Plan

Additional test cases:

- Foreground detection disagreement (`findAppWindowRoot` vs `rootInActiveWindow`) -> fail-secure
- `ScreenshotResult` distinction (`PrivacyBlocked` vs `Failed`)
- Notification filtering from privacy-listed app notifications
- Split-screen one-private-one-public -> blocked
- Privacy block counter increments with source labels
- `click_element` in privacy mode returns blocked result

### Fix 8: `takeScreenshot()` Split-Screen Coverage

`takeScreenshot()` should use the same all-visible-windows policy as `getScreenContent()` so split-screen screenshots do not leak private app content.
