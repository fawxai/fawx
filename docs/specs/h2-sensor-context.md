# H2.8 Sensor Context Spec

*Inject device state into the agent's system prompt so it can make context-aware decisions.*

**Issue:** The agent has no awareness of battery level, network connectivity, GPS location, or local time zone. This leads to poor decisions like starting 20-step tasks at 5% battery or attempting cloud-dependent tasks offline.

**Source:** `agentic-loop-v2.md §12.5`

---

## Design

### 1. SensorContext Data Class

A lightweight snapshot of device state, gathered once per task start (not per tool call).

```kotlin
package ai.citros.core

/**
 * Device network connectivity type.
 */
enum class NetworkType { WIFI, CELLULAR, OFFLINE }

import java.time.ZonedDateTime

/**
 * Snapshot of device sensor state for prompt injection.
 * Gathered once per task start to avoid per-tool-call overhead.
 */
data class SensorContext(
    /** Battery percentage (0-100), or null if unavailable. */
    val batteryPercent: Int? = null, // Must be 0..100 range, or null. Provider clamps invalid values.,
    /** True if device is currently charging. */
    val isCharging: Boolean? = null,
    /** Network type: "wifi", "cellular", "offline", or null if unknown. */
    val networkType: NetworkType? = null,
    /** Coarse location string (city/region), or null if unavailable/denied. */
    val location: String? = null, // Format: "${locality}, ${adminArea}" with fallbacks to adminArea only, then country, then null,
    /** Device local time with timezone. */
    val localTime: ZonedDateTime? = null
) {
    /**
     * Format as a single-line prompt prefix.
     * Only includes fields that are non-null.
     * Example: "Device: battery=72% (charging) | wifi | location=\"Denver, CO\" | 4:15 PM MST"
     */
    fun toPromptLine(): String {
        val parts = mutableListOf<String>()

        batteryPercent?.let { pct ->
            val chargingStr = if (isCharging == true) " (charging)" else ""
            parts.add("battery=${pct}%$chargingStr")
        }

        networkType?.let { parts.add(it.name.lowercase()) }

        location?.let { parts.add(it) }

        localTime?.let { time ->
            val formatter = java.time.format.DateTimeFormatter.ofPattern("h:mm a z")
            parts.add(time.format(formatter))
        }

        return if (parts.isEmpty()) ""
        else "Device: ${parts.joinToString(" | ")}"
    }
}
```

### 2. SensorProvider Interface

Abstraction layer so tests can mock sensor data and production code uses Android APIs.

```kotlin
package ai.citros.core

/**
 * Device network connectivity type.
 */
enum class NetworkType { WIFI, CELLULAR, OFFLINE }

/**
 * Provides device sensor context. Production implementation uses
 * Android BatteryManager, ConnectivityManager, and LocationManager.
 * Test implementation returns fixed values.
 */
interface SensorProvider {
    /** 
     * Gather current sensor snapshot. Should be fast (<100ms).
     * MUST NOT throw exceptions - all errors handled internally with null fields.
     */
    suspend fun snapshot(): SensorContext
}
```

**Production implementation** (`AndroidSensorProvider`) lives in `:chat` module (has Android Context access):
- `BatteryManager.getIntProperty(BATTERY_PROPERTY_CAPACITY)` for battery %
- `BatteryManager.isCharging` for charge state
- `ConnectivityManager.getNetworkCapabilities()` for wifi/cellular/offline
- `Geocoder` reverse geocode from `LocationManager.getLastKnownLocation()` for coarse city
- `ZonedDateTime.now()` for local time with timezone

**No runtime permissions required** for battery and network. Location requires `ACCESS_COARSE_LOCATION` — if not granted, `location` field is null (graceful degradation).

### 3. Prompt Integration

Extend `buildRuntimeSection()` in `PhoneAgentPrompts` to accept an optional `SensorContext`:

```kotlin
private fun buildRuntimeSection(
    phoneControlAvailable: Boolean,
    modelName: String?,
    modelTier: ModelTier,
    mode: PromptMode,
    sensorContext: SensorContext? = null
): String? {
    if (mode == PromptMode.NONE) return null
    val modelId = modelName?.takeIf { it.isNotBlank() } ?: "unknown"
    val accessibility = if (phoneControlAvailable) "enabled" else "disabled"
    val timestamp = Instant.now().atOffset(ZoneOffset.UTC)
        .format(DateTimeFormatter.ISO_OFFSET_DATE_TIME)
    val runtimeParts = mutableListOf(
        "Runtime: model=$modelId",
        "tier=$modelTier",
        "accessibility=$accessibility",
        "time=$timestamp"
    )

    val sensorLine = sensorContext?.toPromptLine()?.takeIf { it.isNotBlank() }
    if (sensorLine != null) {
        runtimeParts.add(sensorLine)
        sensorContext.localTime?.toInstant()?.let { capturedAt ->
            val ageSeconds = (Instant.now().epochSecond - capturedAt.epochSecond).coerceAtLeast(0)
            runtimeParts.add("sensor_age_sec=$ageSeconds")
        }
    }

    return runtimeParts.joinToString(" | ")
}
```

### 4. Injection Point

`PhoneAgentApi` gathers `SensorContext` once when starting a new task (in `chat()` or `chatWithTools()`), then passes it through to prompt building. It is NOT refreshed per tool call — sensor state at task start is sufficient.

```kotlin
class PhoneAgentApi(
    // ... existing params
    private val sensorProvider: SensorProvider? = null
) {
    suspend fun chat(conversation: Conversation): Result<String> {
        val sensors = sensorProvider?.snapshot()
        val systemPrompt = buildSystemPrompt(
            // ... existing params
            sensorContext = sensors
        )
        // ...
    }
}
```

### 4.1 Privacy Control (Required)

Sensor-context prompt injection is **user-controlled** and defaults to disabled:

- Settings path: `Settings → Trust Level → Send device context to cloud models`
- Default: OFF
- When OFF: no battery/network/location/time sensor metadata is injected into prompts
- When ON: prompt injection is enabled and still degrades gracefully when permissions/data are unavailable

### 5. Agent Behavior Rules

Device awareness rules are injected via a new `buildDeviceAwarenessSection()` method in `PhoneAgentPrompts`:

```
## Device Awareness
- If battery is below 15%, warn the user before starting multi-step tasks.
- If offline, do not attempt web_search, web_fetch, or web_browse.
- Use location context to enhance local queries ("nearby", "around here").
- Respect local time for time-sensitive queries.
- Do not proactively tell the user their device state unless they ask or it\'s directly relevant to the task.
```

**Injection Points:** Two separate injection points from this feature:
1. **Strategy section**: Device awareness rules (via `buildDeviceAwarenessSection()`)
2. **Runtime section**: Runtime metadata line with inline sensor fields and `sensor_age_sec` (via extended `buildRuntimeSection()`)

Both are only injected when `sensorContext` is non-null AND has at least one populated field.

---

## Pressure Test

### Edge Cases
1. **No permissions granted**: All `SensorContext` fields are null → `toPromptLine()` returns empty → no sensor line in prompt. Zero behavioral change.
2. **Partial data**: Battery available but location denied → only battery shown. Graceful per-field degradation.
3. **Airplane mode**: `networkType = "offline"`. Agent should avoid web tools.
4. **Location stale**: If `getLastKnownLocation()` is older than 1 hour, location is dropped (`null`) instead of injected.
5. **Battery at exactly 0%**: Edge — device is about to die. Agent should refuse multi-step tasks.
6. **Charging state flips mid-task**: Irrelevant — we snapshot once at task start.
7. **Timezone changes**: `ZonedDateTime.now()` captures current timezone. If user crosses timezone mid-task, stale but acceptable.
8. **Emulator/test device**: May not have battery/location APIs. All fields null → no injection.

### Performance
- Battery + network: synchronous, <1ms each
- Location: `getLastKnownLocation()` is cached, <10ms. No active GPS fix.
- Total overhead: <15ms per task start. Zero per-tool-call overhead.

### Token Budget
- Sensor line: ~15-25 tokens ("Device: battery=72% (charging) | wifi | location=\"Denver, CO\" | 4:15 PM MST")
- Device awareness rules: ~50-60 tokens
- Total: ~75-85 tokens added to system prompt. Negligible.

### Security
- Location is coarse (city-level only via `ACCESS_COARSE_LOCATION`)
- No PII beyond city name, which is already implicit in many user queries
- Sensor data stays in system prompt, never sent as tool results or logged separately

---

## Test Plan

1. **SensorContextTest** — Unit tests for `toPromptLine()`:
   - All fields populated → full line
   - All fields null → empty string
   - Battery only → "Device: battery=72%"
   - Battery + charging → "Device: battery=72% (charging)"
   - Network only → "Device: wifi"
   - Offline → "Device: offline"
   - Location only → "Device: location=\"Denver, CO\""
   - Time only → "Device: 4:15 PM MST"
   - Various combinations
   - **Boundary values**: batteryPercent = 0, 100, 15, -1, 101

2. **PhoneAgentPromptsTest** — Runtime section with sensor context:
   - `sensorContext = null` → same as before (no regression)
   - `sensorContext` with data → runtime line + sensor line
   - `sensorContext` all null → only runtime line (no empty sensor line)
   - **Device awareness section injection** based on sensor data presence

3. **SensorProvider mock** for PhoneAgentApi tests:
   - **Verify `snapshot()` called once** per task, not per tool call (mock + assertion)
   - Verify sensor data reaches the system prompt

4. **AndroidSensorProvider** — Instrumented tests only (device/emulator required):
   - Real Android API integration
   - **Thread safety**: Uses Dispatchers.IO for blocking sensor calls
   - Error handling: exceptions → null fields

---

## Dependency Injection

**Manual construction** in `ChatActivity`/`ChatViewModel`. No DI framework.

```kotlin
val sensorProvider = AndroidSensorProvider(applicationContext)
val phoneAgentApi = PhoneAgentApi(
    // ... existing params
    sensorProvider = sensorProvider
)
```

## Files Changed

| File | Change |
|------|--------|
| `core/SensorContext.kt` | NEW — data class + `toPromptLine()` |
| `core/SensorProvider.kt` | NEW — interface |
| `core/PhoneAgentPrompts.kt` | Extend `buildRuntimeSection()` + `buildSystemPrompt()` signature |
| `core/PhoneAgentApi.kt` | Accept `SensorProvider?`, call `snapshot()` at task start |
| `chat/AndroidSensorProvider.kt` | NEW — production implementation using Android APIs |
| `chat/ChatActivity.kt` | Wire `AndroidSensorProvider` into `ChatViewModel` at app startup |
| `chat/ChatViewModel.kt` | Store `SensorProvider`, pass into `PhoneAgentApi`, rebuild backends when updated |
| `core/test/SensorContextTest.kt` | NEW — unit tests for formatting |
| `core/test/PhoneAgentPromptsTest.kt` | Add sensor context test cases |

## OpenClaw Comparison

N/A — OpenClaw has no sensor context concept. This is phone-agent specific.
