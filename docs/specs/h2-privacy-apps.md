# H2.6 Privacy-Sensitive App Handling Spec

*Selective screen blindness for apps on a user-configured privacy list.*

**Issue:** The agent can see everything on screen, including banking apps, health data, password managers, and private messages. Users need confidence that sensitive app content isn't being sent to cloud LLMs.

**Source:** `agentic-loop-v2.md §12.6`, `SPEC.md §6.1`

---

## Design

### 1. PrivacyList — User-Configurable App Blocklist

Stored in SharedPreferences as a set of Android package names.

```kotlin
package ai.citros.core

/**
 * Manages the set of apps whose screen content should be hidden from the agent.
 * Privacy-listed apps still receive blind actions but screen reads are suppressed.
 */
interface PrivacyList {
    /** Check if the given package is on the privacy list. */
    fun isPrivate(packageName: String): Boolean

    /** Get all packages on the privacy list. */
    fun getAll(): Set<String>

    /** Add a package to the privacy list. */
    fun add(packageName: String)

    /** Remove a package from the privacy list. */
    fun remove(packageName: String)
}
```

**Production implementation** (`SharedPrefsPrivacyList`) in `:chat` module:
- Key: `privacy_app_list` → `Set<String>` of package names
- Default: empty set (no apps blocked)
- Suggested defaults shown in settings UI (not auto-enabled): `com.chase.sig.android`, `com.google.android.apps.authenticator2`, `com.onepassword.android`, etc.

### 2. ScreenReader Integration

The privacy check hooks into `getScreenContent()` — the single method the agent uses to read the screen.

```kotlin
// In ScreenReader.kt (singleton object)
object ScreenReader {
    fun configurePrivacyList(list: PrivacyList?) {
        // set once at app/service startup; read on every screen access
    }

    fun getScreenContent(): ScreenContent {
        val svc = getService() ?: return ScreenContent.empty()
        val pkg = detectForegroundPackage(svc)

        if (pkg != null && privacyList?.isPrivate(pkg) == true) {
            return ScreenContent(
                packageName = pkg,
                elements = emptyList(),
                privacyMode = true
            )
        }

        // ... existing screen reading logic ...
    }
}
```

### 3. ScreenContent Extension

Add `privacyMode` flag to `ScreenContent`:

```kotlin
data class ScreenContent(
    val packageName: String?,
    val elements: List<ScreenElement>,
    val privacyMode: Boolean = false
) {
    fun toToolResult(): String {
        if (privacyMode) {
            return "SCREEN: [Privacy mode — screen content hidden for private_app. " +
                   "Ask the user for guidance if needed.]"
        }
        // ... existing formatting ...
    }
}
```

### 4. Screenshot Suppression

Privacy mode also blocks `takeScreenshot()`:

```kotlin
suspend fun takeScreenshot(): ScreenshotResult {
    val svc = getService() ?: return ScreenshotResult.Failed("Accessibility service unavailable")
    val pkg = detectForegroundPackage(svc)

    if (pkg != null && privacyList?.isPrivate(pkg) == true) {
        Log.d(TAG, "privacy_block source=screenshot blocked=true")
        return ScreenshotResult.PrivacyBlocked
    }

    // ... existing screenshot logic ...
}
```

### 5. Foreground Package Detection

Already partially exists in `getScreenContent()` via `findAppWindowRoot()`. Extract into a reusable method:

```kotlin
/**
 * Detect the package name of the foreground app.
 * Uses AccessibilityService window info (no UsageStatsManager permission needed).
 */
internal fun detectForegroundPackage(svc: AccessibilityService): String? {
    val root = findAppWindowRoot(svc) ?: return null
    return root.packageName?.toString()
}
```

This uses the accessibility tree's `packageName` from the active window — no additional permissions required.

### 6. Agent Behavior

When the agent encounters a privacy-mode screen:

1. Agent sees: `"SCREEN: [Privacy mode — screen content hidden for private_app. Ask the user for guidance if needed.]"`
2. Agent CAN still execute:
   - `press_back` — navigate away
   - `press_home` — go home
   - `swipe` — blind navigation
   - `type` — blind text entry (user directed)
3. Agent CANNOT:
   - `read_screen` — returns privacy message
   - `take_screenshot` — returns `ScreenshotResult.PrivacyBlocked`
   - `click_element` — element IDs unavailable (elements list is empty)

The agent should recognize the privacy message and ask the user what to do rather than attempting blind interactions.

---

## Error Handling

- **`privacyList` is null**: No privacy filtering. Behaves exactly as today. Zero regression.
- **`detectForegroundPackage()` returns null**: Can't determine foreground app → allow screen reading (fail-open for usability, but safe because unknown apps aren't on the list).
- **App switches mid-read**: The privacy check happens BEFORE reading the screen. If the app changes between check and read, the worst case is reading one frame of a non-private app when user was switching away from a private one — acceptable.
- **Privacy list changes mid-task**: Takes effect on the next `getScreenContent()` call. No restart needed.

---

## Pressure Test

### Edge Cases
1. **Empty privacy list**: No apps blocked. Zero behavioral change from today.
2. **System UI package**: `com.android.systemui` — when any privacy apps are configured, `systemui` is intentionally blocked as a conservative policy because notification shade content can contain private-app text while obscuring the source package.
3. **Citros itself**: `ai.citros.chat` on privacy list → agent can't see its own UI. Unusual but allowed.
4. **Multiple windows**: Split-screen with one private and one public app. Privacy mode blocks if **any visible application window** is private (fail-secure), even when focus is on a public window.
5. **Overlay apps**: Privacy-listed app under Citros overlay. `findAppWindowRoot()` already selects the best non-overlay window. Privacy check applies to that window's package.
6. **Notification from private app**: Notifications appear in system UI, not the app's window. When privacy list is non-empty, `com.android.systemui` is blocked so notification text from private apps is not exposed.
7. **App with no package name**: Extremely rare. `packageName = null` → not matched → screen readable. Correct.
8. **WebView inside private app**: WebView elements belong to the app's package. Privacy mode blocks them too. Correct.

### Performance
- `isPrivate()` on SharedPreferences `Set<String>`: O(1) hash lookup, <1ms
- `detectForegroundPackage()`: reuses existing `findAppWindowRoot()`, already called in current flow, <5ms
- Total added overhead: ~1ms per screen read. Negligible.

### Token Budget
- Privacy message: ~25 tokens (vs. hundreds for a full screen read)
- Actually SAVES tokens when privacy apps are active

### Security
- Privacy check is client-side enforcement — can't be bypassed by prompt injection
- Screen content never enters the ScreenContent object in privacy mode (not just hidden in output)
- No content is collected then filtered — collection is prevented at source
- Package names on the privacy list are NOT sent in the system prompt (only used for local matching)
- Outbound agent/tool/verifier strings must never include raw package names for blocked apps.
  Use a fixed redaction token (`private_app`) when a privacy block reason is needed.
- Allowed off-device metadata for privacy blocks is limited to source type (`read_screen`, `screenshot`, `action`) and blocked/not-blocked status.

---

## Test Plan

1. **PrivacyListTest** (unit):
   - Empty list → `isPrivate()` returns false for any package
   - Add package → `isPrivate()` returns true
   - Remove package → `isPrivate()` returns false again
   - Multiple packages → independent add/remove
   - Null/empty package name handling

2. **ScreenReaderTest** — Privacy mode integration:
   - `privacyList = null` → normal screen reading (no regression)
   - Private app in foreground → `ScreenContent.privacyMode = true`, empty elements
   - Non-private app → normal screen reading
   - Private app → `toToolResult()` returns privacy message
   - Private app → `takeScreenshot()` returns `ScreenshotResult.PrivacyBlocked`
   - Unknown foreground package (null) → normal screen reading

3. **ScreenContentTest** — Privacy formatting:
   - `privacyMode = true` → correct privacy message string
   - `privacyMode = false` → normal formatting (no regression)
   - Privacy message redacts package names with `private_app`

4. **Integration** (PhoneAgentApi/AgentExecutor):
   - Agent tool loop with private app → receives privacy message → doesn't retry screen read
   - Agent can still execute blind actions (press_back, press_home) on private apps
   - Privacy mode doesn't break tool loop or stuck detection

## Dependency Injection

Manual startup wiring via singleton configuration. `SharedPrefsPrivacyList` is created with `applicationContext` and injected through `ScreenReader.configurePrivacyList(...)` (or `ScreenReader.attach(service, privacyList)` for atomic accessibility-service startup).

```kotlin
// ChatActivity startup
ScreenReader.configurePrivacyList(SharedPrefsPrivacyList(applicationContext))

// AccessibilityService startup (atomic attach + privacy config)
ScreenReader.attach(this, SharedPrefsPrivacyList(applicationContext))
```

---

## Files Changed

| File | Change |
|------|--------|
| `core/PrivacyList.kt` | NEW — interface |
| `core/ScreenReader.kt` | Add privacy check in `getScreenContent()` + `takeScreenshot()`, extract `detectForegroundPackage()` |
| `core/ScreenContent.kt` | Add `privacyMode` field + privacy message formatting |
| `chat/SharedPrefsPrivacyList.kt` | NEW — production implementation |
| `core/test/ScreenReaderTest.kt` | Add privacy mode test cases |
| `core/test/ScreenContentTest.kt` | Add privacy formatting tests |
| `core/test/PrivacyListTest.kt` | NEW — unit tests |

## OpenClaw Comparison

N/A — OpenClaw has no screen reading capability. This is phone-agent specific. OpenClaw does have tool allowlists (`tools.allow`) which is a similar concept applied at the tool level rather than the observation level.

---

## Review Notes

Implementation/review history is tracked in `docs/specs/h2-privacy-apps-review-notes.md` to keep this spec normative and stable.
