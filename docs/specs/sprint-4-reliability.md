# Sprint 4 Stream B: Reliability Hardening

*Fix the workflows that break in the first 10 minutes.*

**Status:** Spec
**Issues:** #647, #638, #663, #751, #698, #779, #780
**Prerequisite:** Sprints 0-3 complete (service architecture, loop tuning, playbooks, safety layer)
**Estimated PRs:** 4-5
**Node:** MacBook Pro

---

## Problem

Three user-facing bugs and a fragile test infrastructure undermine confidence in the product:

1. **Google Maps stuck on suggestion dropdowns (#647).** When the agent types a search query in Google Maps, autocomplete suggestions appear as a dropdown overlay. The agent tries to tap a suggestion but the dropdown dismisses on any screen change, creating an infinite retry loop. This is a top-3 workflow — directions, restaurant search, nearby places.

2. **Third-party text input garbles output (#638).** `input text` via ADB works for simple fields but fails with apps that have custom input methods, autocomplete, or suggestion strips. The agent types "Hello world" and the field shows "Helloworld" or "Hello worl d" or the autocomplete replaces the text entirely. Affects messaging apps, search bars, form fields.

3. **Voice input stops on clean install (#663).** SherpaOnnx (on-device STT) fails to initialize on first launch because model files haven't been downloaded yet. The fallback to Android's built-in STT doesn't trigger — voice input just silently stops. First-run experience killer.

4. **Test infrastructure gaps.** CI gate weakened (#751), no sensor CI coverage (#698), and Sprint 3 backlog items (#779 OTP regex, #780 policy concurrency) erode safety confidence.

---

## Solution

### PR 1: Google Maps Navigation Fix (#647)

**Root cause:** The autocomplete dropdown is a separate UI layer that dismisses when accessibility focus changes. The agent's screen read triggers a focus change, dismissing the dropdown before it can tap a suggestion.

**Approach: Three-tier search submission (IME primary, text match middle, spatial fallback).**

Instead of: type query → read screen → find suggestion → tap suggestion
Do: type query → submit via IME search action → verify results. Fall back through progressively less robust tiers only if needed.

```kotlin
// In PhoneAgentApi or tool execution layer
class MapsSuggestionStrategy(private val resources: Resources) {

    // Density-aware offset for spatial fallback (48dp — standard list item height).
    // Assumption: first suggestion appears ~48dp below the search field bottom
    // on stock Maps. This varies by device density and Maps version.
    private val suggestionOffsetDp = 48
    private fun dpToPx(dp: Int): Int =
        (dp * resources.displayMetrics.density + 0.5f).toInt()

    /**
     * Tier 1 (Primary): Submit search via IME action.
     * Most robust — device-independent, no coordinate assumptions.
     * Type query → press Enter / dispatch IME_ACTION_SEARCH → verify results.
     *
     * Tier 2 (Middle): Find suggestion node by text match.
     * Use findAccessibilityNodeInfosByText(query) to locate a suggestion
     * node without a full screen read (avoids focus-change dismissal).
     *
     * Tier 3 (Fallback): Density-aware spatial tap.
     * Tap at a dp-based offset below the search field. Last resort.
     */
    fun handleMapsSuggestion(
        searchField: AccessibilityNodeInfo,
        query: String
    ): ToolResult {
        // --- Tier 1: IME search action (primary) ---
        val imeArgs = Bundle().apply {
            putInt(
                AccessibilityNodeInfo.ACTION_ARGUMENT_IME_ACTION_ID,
                EditorInfo.IME_ACTION_SEARCH
            )
        }
        searchField.performAction(AccessibilityNodeInfo.ACTION_IME_ENTER, imeArgs)
        Thread.sleep(1500)
        val screen1 = readScreen()
        if (screen1.containsMapResult()) return ToolResult("Search submitted via IME", false)

        // --- Tier 2: Find suggestion by text match (middle) ---
        val rootNode = searchField.window?.root
        if (rootNode != null) {
            val suggestions = rootNode.findAccessibilityNodeInfosByText(query)
            val suggestionNode = suggestions.firstOrNull {
                it != searchField && it.isClickable
            }
            if (suggestionNode != null) {
                suggestionNode.performAction(AccessibilityNodeInfo.ACTION_CLICK)
                Thread.sleep(1000)
                val screen2 = readScreen()
                if (screen2.containsMapResult()) return ToolResult("Suggestion tapped via text match", false)
            }
        }

        // --- Tier 3: Density-aware spatial tap (fallback) ---
        val offsetPx = dpToPx(suggestionOffsetDp)
        val suggestY = searchField.boundsInScreen.bottom + offsetPx
        val suggestX = searchField.boundsInScreen.centerX()
        performTap(suggestX, suggestY)
        Thread.sleep(1000)
        val screen3 = readScreen()
        if (screen3.containsMapResult()) return ToolResult("Navigation via spatial tap", false)

        // --- Final fallback: raw Enter key ---
        performKeyPress(KeyEvent.KEYCODE_ENTER)
        return ToolResult("Search submitted via Enter fallback", false)
    }
}
```

**App-version resilience:** The primary approach (IME search action) is completely device- and app-version-independent. Tier 2 (text match) avoids coordinate assumptions. Tier 3 (spatial tap) uses density-aware dp offsets (not hardcoded pixels) so it scales across screen densities, though it remains sensitive to Maps layout changes. The Enter key final fallback ensures the query always submits.

**Test strategy:**
- Unit test: `MapsSuggestionStrategy` Tier 1 (IME action dispatched, result verified)
- Unit test: Tier 2 (`findAccessibilityNodeInfosByText` returns clickable suggestion)
- Unit test: Tier 3 (spatial tap with density conversion — verify dp→px math at 1x, 2x, 3x densities)
- Unit test: Full fallback chain (Tier 1 fails → Tier 2 fails → Tier 3 fails → Enter key)
- Integration test: scripted screen state with dropdown-present and dropdown-absent scenarios
- Regression scenario: "Navigate to Times Square" end-to-end in harness

---

### PR 2: Robust Text Input Fallback Chain (#638)

**Root cause:** `adb shell input text` has known limitations: no space handling in some apps, conflicts with IME predictions, no Unicode support. The agent has one input method and no fallback.

**Approach: Three-tier input fallback chain.**

```
Tier 1: AccessibilityNodeInfo.setText()
  ↓ (fails or text doesn't match)
Tier 2: Clipboard paste (set clipboard + paste event)
  ↓ (fails or no paste support)
Tier 3: Character-by-character key events
  ↓ (last resort, slow but universal)
Tier 4: ADB input text (current behavior, kept as final fallback)
```

```kotlin
class RobustTextInput(private val screenReader: ScreenReader) {
    /**
     * Input text using the most reliable available method.
     * Verifies the result after each attempt and falls back on mismatch.
     */
    suspend fun inputText(
        node: AccessibilityNodeInfo,
        text: String,
        verify: Boolean = true
    ): InputResult {
        // Tier 1: Direct setText via accessibility
        if (node.isEditable) {
            val args = Bundle().apply { putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text) }
            node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
            if (!verify || verifyText(node, text)) return InputResult.SUCCESS
        }

        // Tier 2: Clipboard paste
        // Skip for password fields — clipboard exposes text to other apps
        val isPasswordField = (node.inputType and InputType.TYPE_TEXT_VARIATION_PASSWORD) != 0
                || (node.inputType and InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD) != 0
                || (node.inputType and InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD) != 0
        if (!isPasswordField) {
            setClipboard(text)
            node.performAction(AccessibilityNodeInfo.ACTION_PASTE)
            // Clear clipboard immediately to prevent exposure to other apps
            clipboardManager.setPrimaryClip(ClipData.newPlainText("", ""))
            if (!verify || verifyText(node, text)) return InputResult.SUCCESS
        }

        // Tier 3: Character-by-character key events
        // Adaptive delay: start at 30ms, increase to 100ms if verification fails.
        // Range chosen empirically: 30ms works for most IMEs, 100ms handles
        // heavy prediction/autocomplete engines. Beyond 100ms indicates a
        // fundamental input issue — fall through to Tier 4.
        clearField(node)
        var charDelayMs = 30L
        text.forEach { char ->
            dispatchKeyEvent(char)
            delay(charDelayMs)
        }
        if (verify && !verifyText(node, text) && charDelayMs < 100L) {
            // Retry with slower delay
            charDelayMs = 100L
            clearField(node)
            text.forEach { char ->
                dispatchKeyEvent(char)
                delay(charDelayMs)
            }
        }
        if (!verify || verifyText(node, text)) return InputResult.SUCCESS

        // Tier 4: ADB fallback
        clearField(node)
        adbInputText(text)
        return InputResult.FALLBACK
    }

    private fun verifyText(node: AccessibilityNodeInfo, expected: String): Boolean {
        val actual = node.text?.toString() ?: return false
        return actual.trim() == expected.trim()
    }
}
```

**Key design decisions:**
- Each tier verifies the result before declaring success — no silent corruption
- Tier 2 (clipboard) clears clipboard immediately after paste to prevent exposure to other apps; skipped entirely for password fields (checked via `inputType` flags)
- Tier 3 (character-by-character) uses adaptive delay: starts at 30ms/char, retries at 100ms if verification fails — balances speed vs. IME compatibility
- The `verify` flag allows skipping verification for performance-sensitive paths
- Existing `type_text` tool becomes a thin wrapper around `RobustTextInput`

**Test strategy:**
- Unit test: each tier in isolation with mock AccessibilityNodeInfo
- Integration test: fallback chain with tier 1 failure → tier 2 → verify
- Regression scenario: "Send 'Hello World' in WhatsApp" end-to-end

---

### PR 3: Voice Input Clean Install Fix (#663)

**Root cause:** `SherpaOnnxManager` initialization fails when model files aren't present. The `catch` block logs the error but doesn't trigger the Android STT fallback path.

**Approach: Explicit fallback with state machine.**

```kotlin
enum class SttState { INITIALIZING, SHERPA_READY, AWAITING_CONSENT, ANDROID_FALLBACK, FAILED }

/** User's persisted choice for cloud STT fallback. */
enum class CloudSttConsent { UNASKED, ACCEPTED, DECLINED_TYPE, DECLINED_WAIT }

class SttManager(
    private val context: Context,
    private val prefs: SharedPreferences
) {
    private var state: SttState = SttState.INITIALIZING

    /** Persisted consent for cloud STT. */
    var cloudConsent: CloudSttConsent
        get() = CloudSttConsent.valueOf(
            prefs.getString("cloud_stt_consent", "UNASKED") ?: "UNASKED"
        )
        private set(value) = prefs.edit().putString("cloud_stt_consent", value.name).apply()

    /**
     * Initialize STT with explicit user consent for cloud fallback.
     *
     * Privacy contract:
     * - SherpaOnnx runs entirely on-device (no data leaves the phone).
     * - Android SpeechRecognizer sends audio to Google's servers.
     * - We NEVER silently switch to cloud STT. The user must explicitly consent.
     *
     * Flow:
     * 1. Try SherpaOnnx (on-device, privacy-preserving)
     * 2. If unavailable AND Android STT exists → enter AWAITING_CONSENT
     *    (unless user already consented/declined in a previous session)
     * 3. If both fail → FAILED
     */
    suspend fun initialize(): SttState {
        state = try {
            SherpaOnnxManager.initialize(context)
            SttState.SHERPA_READY
        } catch (e: Exception) {
            Log.w(TAG, "SherpaOnnx unavailable", e)
            if (!isAndroidSttAvailable(context)) {
                SttState.FAILED
            } else when (cloudConsent) {
                CloudSttConsent.ACCEPTED -> SttState.ANDROID_FALLBACK
                CloudSttConsent.DECLINED_TYPE, CloudSttConsent.DECLINED_WAIT -> SttState.FAILED
                CloudSttConsent.UNASKED -> SttState.AWAITING_CONSENT
            }
        }
        // Kick off background model download so SherpaOnnx becomes available later
        if (state != SttState.SHERPA_READY) {
            SherpaOnnxManager.downloadModelsAsync(context)
        }
        return state
    }

    /**
     * Present the consent choice to the user. Called by the UI layer
     * when state == AWAITING_CONSENT.
     *
     * Message shown to user:
     *   "On-device speech recognition isn't ready yet.
     *    I can use cloud speech recognition instead (audio is sent to Google)."
     *
     * Options:
     *   [Use cloud STT] → ACCEPTED → state becomes ANDROID_FALLBACK
     *   [Type instead]  → DECLINED_TYPE → state becomes FAILED (voice disabled)
     *   [Wait for download] → DECLINED_WAIT → state becomes FAILED (retry later)
     *
     * The choice is persisted so the user is not asked again.
     */
    fun handleConsentChoice(choice: CloudSttConsent) {
        cloudConsent = choice
        state = when (choice) {
            CloudSttConsent.ACCEPTED -> SttState.ANDROID_FALLBACK
            else -> SttState.FAILED
        }
    }

    /**
     * Start listening using whichever engine is available.
     * Shows a persistent cloud indicator (☁) when using Android STT.
     */
    fun startListening(callback: SttCallback) {
        when (state) {
            SttState.SHERPA_READY -> sherpaListen(callback)
            SttState.ANDROID_FALLBACK -> {
                showCloudSttIndicator() // Persistent small cloud icon in status bar
                androidSttListen(callback)
            }
            SttState.AWAITING_CONSENT -> callback.onError("Waiting for user consent")
            SttState.FAILED -> callback.onError("No speech recognition available")
            SttState.INITIALIZING -> callback.onError("STT not initialized")
        }
    }

    /** Called when SherpaOnnx model download completes — auto-upgrade. */
    fun onModelsDownloaded() {
        if (state == SttState.ANDROID_FALLBACK || state == SttState.FAILED) {
            try {
                SherpaOnnxManager.initialize(context)
                state = SttState.SHERPA_READY
                hideCloudSttIndicator()
            } catch (e: Exception) { /* stay on current engine */ }
        }
    }
}
```

**Key design decisions:**
- State machine makes the current engine explicit and testable
- Fallback requires **explicit user consent** — never silently switch to cloud STT
- Three consent options: use cloud / type instead / wait for download — persisted so user is not re-prompted
- Persistent cloud icon (☁) visible whenever Android STT is active, so the user always knows audio is leaving the device
- Background model download kicks off after fallback, auto-upgrades to on-device when ready

**Test strategy:**
- Unit test: `SttManager` state transitions with SherpaOnnx failure injection
- Unit test: AWAITING_CONSENT state when SherpaOnnx fails and consent not yet given
- Unit test: consent acceptance → ANDROID_FALLBACK, consent decline → FAILED
- Unit test: consent persistence across SttManager re-initialization
- Unit test: FAILED state when both engines are unavailable
- Unit test: auto-upgrade from ANDROID_FALLBACK to SHERPA_READY on model download
- Integration test: cloud STT indicator shown/hidden on state transitions
- Regression scenario: fresh install voice input in harness (consent flow exercised)

---

### PR 4: Regression Suite Hardening

The Sprint 1 regression harness exists as a skeleton. This PR adds real scenarios for the top workflows.

**Scenarios to implement:**

| # | Scenario | Validates | Tools Exercised |
|---|----------|-----------|-----------------|
| 1 | "Open Settings" | Basic app launch | `open_app` |
| 2 | "Navigate to Times Square" | Maps + suggestion handling | `open_app`, `tap`, `type_text`, Maps fix |
| 3 | "Send 'Hello' to Mom in Messages" | Text input + messaging | `open_app`, `tap`, `type_text`, text input chain |
| 4 | "Take a screenshot" | Screen capture | `screenshot` |
| 5 | "What's on my screen?" | Screen reading | `read_screen` |
| 6 | "Set a timer for 5 minutes" | Clock app | `open_app`, `tap`, `type_text` |
| 7 | "Search for pizza near me" | Web search | `web_search`, `read_screen` |
| 8 | "Turn on Do Not Disturb" | System settings | `open_app`, `tap`, `scroll` |
| 9 | "Open the camera and take a photo" | Camera | `open_app`, `tap` |
| 10 | "Read my last notification" | Notification access | `read_notifications` |

**Harness requirements:**
- Each scenario defines: task description, expected tool sequence, success criteria, timeout
- Scenarios run against `ScriptedProviderClient` with pre-recorded screen states
- Pass/fail is deterministic (no LLM in the loop during regression)
- Results output as JUnit XML for CI integration

**KPI tracking (new):**
```kotlin
data class RegressionResult(
    val scenario: String,
    val passed: Boolean,
    val toolSteps: Int,
    val wallTimeMs: Long,
    val failureReason: String? = null
)
```

---

#### Manual Pre-Release Regression

Separate from automated CI. Run on a real Pixel device with a real LLM before any release.

- [ ] 1. "Open Settings" — basic app launch
- [ ] 2. "Navigate to Times Square" — Maps + suggestion handling
- [ ] 3. "Send 'Hello' to Mom in Messages" — text input + messaging
- [ ] 4. "Take a screenshot" — screen capture
- [ ] 5. "What's on my screen?" — screen reading
- [ ] 6. "Set a timer for 5 minutes" — Clock app interaction
- [ ] 7. "Search for pizza near me" — web search
- [ ] 8. "Turn on Do Not Disturb" — system settings
- [ ] 9. "Open the camera and take a photo" — camera
- [ ] 10. "Read my last notification" — notification access

**Note:** This is a manual sanity check, not automated CI. It validates the full stack (real LLM decisions, real device, real apps) which scripted regression cannot cover.

---

### PR 5: Test Infrastructure + Safety Backlog

**CI baseline (#751):**
- Identify and fix flaky tests causing CI gate weakness
- Re-enable full Android CI gate on staging
- Add `@Flaky` annotation for known-flaky tests with tracking issue

**Sensor CI (#698):**
- Add timeout and concurrency test suites for `SensorProvider`
- Wire into CI matrix

**Safety backlog:**
- #779: Expand OTP regex in `PolicySummarySanitizer` to cover these specific formats:
  - 6-digit numeric: `"123456"`
  - 8-digit numeric: `"12345678"`
  - Alphanumeric: `"ABC123"`
  - Phrase patterns: `"Your code is X"`, `"Your verification code is: X"`, `"X is your OTP"`
  - Dash-separated: `"123-456"`
  - Space-separated: `"123 456"`
- #780: Run a concurrent load benchmark (100 parallel `ActionPolicy.evaluate()` calls), measure p50/p95/p99 latency. If p99 > 5ms, file a follow-up issue with the profiling data and a proposed solution (e.g., `ConcurrentHashMap`-based lock-free approach). If p99 ≤ 5ms, document the benchmark results in a PR comment and close #780 as "within acceptable bounds for Phase 1 volumes."

---

## Test Matrix

| PR | Unit Tests | Integration Tests | Regression Scenarios |
|----|-----------|------------------|---------------------|
| Maps fix | `MapsSuggestionStrategy` | Dropdown present/absent | "Navigate to Times Square" |
| Text input | Each fallback tier | Fallback chain transitions | "Send Hello in WhatsApp" |
| Voice fix | `SttManager` state machine | Sherpa failure → Android fallback | Fresh install voice |
| Regression suite | Scenario definitions | Harness execution | All 10 scenarios |
| Test infra | Flaky test fixes | CI gate validation | Full staging green |

## Rollout

All bug fixes are **immediate** — no feature flag needed. They fix broken behavior.

Regression suite is **additive** — new tests, no behavior changes.

Test infra changes are **CI-only** — no runtime impact.

---

## Blindspots

1. **Maps spatial fallback (Tier 3) assumes predictable suggestion positioning.** If Google redesigns the Maps search UI, the dp-offset approach breaks. However, Tier 1 (IME search) and Tier 2 (text match) are layout-independent. The spatial tap is a last resort.

2. **Text input Tier 1 (setText) may not trigger TextWatcher callbacks** in some apps, causing downstream logic to miss the input. The verify step catches this, but some apps may show the text while not registering it internally.

3. **Text input Tier 2 (clipboard paste) briefly exposes text to other apps** between `setClipboard()` and the immediate clear. On Android 12+ the system shows a toast when clipboard is accessed, which may confuse users. Tier 2 is skipped for password fields.

4. **Android STT fallback requires network and explicit user consent.** On a device with no connectivity, both STT engines fail. The FAILED state surfaces this, but there's no offline fallback beyond typing.

5. **Regression scenarios with ScriptedProviderClient** test the harness plumbing but not the actual LLM decision-making. The manual pre-release regression checklist (see PR 4) complements this with real-device validation.
