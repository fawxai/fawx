# Pressure Test: Voice I/O MVP (#556)

**Date:** 2026-02-17
**Feature:** Voice Input (STT) + Voice Output (TTS) with interchangeable providers
**Reference:** OpenClaw TTS/STT (`tts-core.ts`, `runner.ts` media-understanding providers)
**Issue:** https://github.com/abbudjoe/fawx/issues/556

---

## 1. How the Reference System (OpenClaw) Implements Voice

### 1.1 TTS (Text-to-Speech) — Output

**Architecture:** Three standalone functions (`elevenLabsTTS`, `openaiTTS`, `edgeTTS`) behind a central orchestrator in `tts.ts`. No interface/class hierarchy — just functions dispatched by a provider string.

**Source-level detail:**
- `elevenLabsTTS(params)` → POST to `/v1/text-to-speech/{voiceId}`, returns `Buffer`
- `openaiTTS(params)` → POST to `/v1/audio/speech`, returns `Buffer`
- `edgeTTS(params)` → `new EdgeTTS({...}).ttsPromise(text, outputPath)`, writes file

**Provider selection:** Config-driven (`messages.tts.provider`). Slash command `/tts provider <name>` for runtime switching. Fallback order: configured → openai (if key) → elevenlabs (if key) → edge.

**Output format negotiation:** Channel-aware. Telegram = Opus (voice bubble UX). Others = MP3. Format is fixed per channel, not per provider.

**Long text handling:** If text > `maxLength` (default 1500 chars), auto-summarize using a cheap model (configurable), then TTS the summary. If no summary model available, truncate.

**Model directives:** The LLM can emit `[[tts:provider=elevenlabs voiceId=xxx]]` and `[[tts:text]]expressive audio text[[/tts:text]]` to override voice per-reply. Parsed by `parseTtsDirectives()` with an allowlist policy.

**Auto modes:** `off`, `always`, `inbound` (reply with voice only when user sends voice), `tagged` (only when `[[tts]]` tags present).

### 1.2 STT (Speech-to-Text) — Input

**Architecture:** Provider registry (`buildProviderRegistry`) with capability-based routing. Each provider implements `transcribeAudio(params) → { text, model }`. Tried in config order (fallback chain).

**Source-level detail:**
- `transcribeOpenAiCompatibleAudio()` → `POST /v1/audio/transcriptions` (multipart form)
- `transcribeDeepgramAudio()` → `POST /v1/listen` (binary body)
- `transcribeGeminiAudio()` → `POST /v1beta/models/{model}:generateContent` (base64 inline_data)
- CLI providers: spawn process (`whisper`, `whisper-cli`, `sherpa-onnx-offline`), capture stdout

**Auto-detection:** When no explicit `models` config, tries local CLIs first → Gemini → provider keys (OpenAI → Groq → Deepgram → Google).

**Preflight transcription:** In group chats with `requireMention`, audio is transcribed *before* mention checking so voice notes that say "@BotName" still trigger replies.

**Data flow:**
```
Voice note → download attachment → enforce maxBytes → try provider[0]
  → fail? → try provider[1] → ... → inject transcript as [Audio] block
  → set {{Transcript}} template var → proceed with normal reply pipeline
```

### 1.3 Key Patterns

| Pattern | OpenClaw | Notes |
|---------|----------|-------|
| Provider abstraction | Functions, not interfaces | Works for server-side (no lifecycle mgmt needed) |
| Config-driven switching | `messages.tts.provider` + slash commands | Runtime hot-swap without restart |
| Fallback chains | Ordered model array for STT; provider cascade for TTS | First success wins |
| Channel-aware output | Telegram=Opus, others=MP3 | Format negotiation at delivery layer |
| Auto-summary | Cheap model summarizes long text before TTS | Prevents silent failures on long responses |
| Model overrides | `[[tts:...]]` directive system | LLM can control voice expressiveness |

---

## 2. How Our Design Compares

### 2.1 Architecture

| Aspect | OpenClaw | Fawx | Why different |
|--------|----------|--------|--------------|
| **Language** | TypeScript (Node.js) | Kotlin (Android) | Platform |
| **Provider abstraction** | Standalone functions | Kotlin interfaces + implementations | Android needs lifecycle management (init/release, Activity binding, permissions). Functions aren't enough. |
| **STT input source** | File-based (downloaded voice note) | Real-time microphone stream | OpenClaw processes pre-recorded files. Fawx needs live mic input with partial results. |
| **TTS output destination** | File → send as attachment | AudioTrack/MediaPlayer playback | OpenClaw sends audio files over messaging. Fawx plays audio locally on the device. |
| **Lifecycle** | Stateless (per-request) | Stateful (Android lifecycle-bound) | `SpeechRecognizer` must be created on main thread, released on destroy. `TextToSpeech` needs async init callback. |
| **Permissions** | N/A (server-side) | `RECORD_AUDIO` runtime permission | Must handle grant/deny/rationale UI flow |
| **Offline capability** | Edge TTS only (needs network for synthesis) | Android built-in APIs work fully offline | On-device = zero latency, zero cost, works in airplane mode |

### 2.2 Proposed Fawx Interfaces

```kotlin
// ── STT ──────────────────────────────────────────────
interface SpeechToTextProvider {
    val providerId: String           // "android", "openai-whisper", "deepgram"
    val displayName: String          // "On-Device", "OpenAI Whisper", "Deepgram Nova"
    val requiresNetwork: Boolean     // false for Android built-in
    val isAvailable: Boolean         // check API key present, engine ready, etc.

    /** One-time init. May be async (TTS engine init, API key validation). */
    suspend fun initialize(context: Context)

    /**
     * Start listening and return a Flow of speech events.
     * Flow emits Partial results as the user speaks, a Final result when done,
     * and Error if something goes wrong. Flow completes after Final or Error.
     *
     * Threading: Flow is collected on Dispatchers.Main (SpeechRecognizer requirement).
     * Callers should use flowOn() if they need to process on a different dispatcher.
     *
     * Cancellation contract: Cancelling the returned Flow automatically calls
     * stopListening() internally (via callbackFlow's awaitClose block).
     * Callers do NOT need to call stopListening() explicitly — just cancel
     * the collection (e.g., via Job.cancel() or scope cancellation).
     * This guarantees no dangling SpeechRecognizer listeners and prevents
     * ERROR_RECOGNIZER_BUSY on the next startListening() call.
     */
    fun startListening(): Flow<SpeechEvent>

    /** Explicitly stop listening (optional — flow cancellation handles this automatically). */
    fun stopListening()
    fun cancel()

    /**
     * Release all resources. Called by VoiceManager.release() — callers should
     * NOT call this directly; instead call VoiceManager.release() from the
     * lifecycle owner's onDestroy().
     */
    fun release()
}

sealed class SpeechEvent {
    data class Partial(val text: String) : SpeechEvent()
    data class Final(val text: String) : SpeechEvent()
    data class Error(val error: SpeechError) : SpeechEvent()
}

// ── TTS ──────────────────────────────────────────────
interface TextToSpeechProvider {
    val providerId: String           // "android", "openai", "elevenlabs"
    val displayName: String
    val requiresNetwork: Boolean
    val isAvailable: Boolean

    suspend fun initialize(context: Context)

    /** Speak text. Returns when playback completes or is interrupted. */
    suspend fun speak(
        text: String,
        options: TtsOptions = TtsOptions()
    )

    /** Stop current playback. */
    fun stop()

    /** Check if currently speaking. */
    val isSpeaking: Boolean

    /**
     * Release all resources. Called by VoiceManager.release() — callers should
     * NOT call this directly; instead call VoiceManager.release() from the
     * lifecycle owner's onDestroy().
     */
    fun release()
}

data class TtsOptions(
    val speed: Float = 1.0f,         // 0.5 - 2.0
    val pitch: Float = 1.0f,         // 0.5 - 2.0
    /** FLUSH cancels in-progress speech (correct for agent responses — always want the latest).
     *  ADD queues behind current speech (useful for multi-part narration, not typical). */
    val queueMode: QueueMode = QueueMode.FLUSH
)

enum class QueueMode { FLUSH, ADD }

sealed class SpeechError {
    data class PermissionDenied(val message: String) : SpeechError()
    data class NetworkError(val message: String) : SpeechError()
    data class EngineError(val message: String) : SpeechError()
    data class Timeout(val message: String) : SpeechError()
    data class Unavailable(val message: String) : SpeechError()
}
```

### 2.3 VoiceManager — Central Coordinator

```kotlin
class VoiceManager(
    private val context: Context,
    private val keyStore: KeyStore
) {
    // Registered providers
    val sttProviders: List<SpeechToTextProvider>
    val ttsProviders: List<TextToSpeechProvider>

    // Active selections exposed as StateFlow for thread-safe observation.
    // Internal writes go through Mutex to prevent races between UI thread
    // (mic button taps, settings changes) and ViewModel coroutines.
    val activeStt: StateFlow<SpeechToTextProvider>
    val activeTts: StateFlow<TextToSpeechProvider>

    // Settings as MutableStateFlow for thread-safe observation from UI + ViewModel.
    // MutableStateFlow uses atomic CAS — reads and writes are safe from any thread.
    // Persisted via SharedPreferences (same pattern as existing wallet/model prefs).
    // SharedPreferences writes happen on background thread (apply() is async).
    val autoSpeakResponses: MutableStateFlow<Boolean>   // default: false
    val autoSendAfterVoice: MutableStateFlow<Boolean>   // default: false

    // Switch at runtime — validates availability before switching.
    // Returns Result.failure with:
    //   - IllegalArgumentException if providerId not found
    //   - IllegalStateException if provider.isAvailable == false
    //   - Exception from provider.initialize() if init fails
    suspend fun switchStt(providerId: String): Result<Unit>
    suspend fun switchTts(providerId: String): Result<Unit>

    /**
     * Release all registered providers. Call from the lifecycle owner's onDestroy().
     * Callers only need to call VoiceManager.release() — it delegates to each provider.
     */
    fun release()
}
```

### 2.4 What's the Same as OpenClaw

- **Provider abstraction layer** — swap backends without changing callers
- **Config/settings-driven selection** — user picks provider in settings, persisted
- **Fallback concept** — if active provider fails, could fall through to next
- **Auto-mode for TTS** — auto-speak responses toggle (analogous to `auto: "always"`)

### 2.5 What's Different (Intentional Divergences)

| Divergence | OpenClaw | Fawx | Reasoning |
|-----------|----------|--------|-----------|
| Interface vs functions | Functions | Kotlin interfaces | Android lifecycle requires init/release. `SpeechRecognizer` must be on main thread. Object lifecycle is mandatory. |
| Real-time vs file-based STT | Process files | Live mic stream with partials | Fawx is a local app — users talk to it, not send voice files |
| Local playback vs file output | Generate file → send | Play audio on device speaker | The phone IS the output device |
| On-device first | Cloud-first (Edge TTS is the free fallback) | Android built-in first (cloud is the upgrade) | Phone-native philosophy. Zero dependency, works offline |
| No directive system (v1) | `[[tts:...]]` model overrides | Not in v1 | Agent doesn't need to control voice yet. Add later if multi-persona needed |
| Permission flow | N/A | Full Android runtime permission UX | `RECORD_AUDIO` is a dangerous permission. Must handle deny, rationale, "don't ask again" |
| Channel format negotiation | Opus/MP3 per channel | N/A (local playback only) | Single output destination (phone speaker). No format negotiation needed. |

---

## 3. Gaps Found

### 3.1 Critical (Must Fix Before Implementation)

**C1: RECORD_AUDIO Permission Flow**
Android requires runtime `RECORD_AUDIO` permission for `SpeechRecognizer`. Must handle:
- First-time request with rationale
- Permanent denial (`shouldShowRequestPermissionRationale` = false) → guide to Settings
- Permission check before every `startListening()` call
- Graceful degradation: mic button disabled/hidden when permission denied

**C2: SpeechRecognizer Lifecycle Fragility**
Android's `SpeechRecognizer` is notoriously fragile:
- Must be created AND called from the main thread
- `ERROR_RECOGNIZER_BUSY` if you call `startListening()` while already active
- No built-in timeout — can hang indefinitely on some devices
- Some OEMs ship broken implementations (Samsung, Xiaomi)
- Network-based recognition may fail silently on airplane mode

*Mitigation:* Wrap in a lifecycle-aware manager with:
- Main-thread enforcement via `Dispatchers.Main`
- State machine: `IDLE → LISTENING → PROCESSING → RESULT/ERROR → IDLE`
- Hard timeout (configurable, default 30s)
- Device-specific workarounds registry (deferred to v2 if needed)

**C3: TextToSpeech Engine Initialization Timing**
`TextToSpeech` constructor is async — it calls `onInit(status)` callback sometime after construction. If you call `speak()` before `onInit`, it silently drops the text.

*Mitigation:* 
- `initialize()` returns `suspend fun` that awaits `onInit` via `suspendCancellableCoroutine`
- Expose `isReady` state
- Queue TTS requests that arrive before init completes
- Handle `ERROR` init status (no TTS engine installed — rare but possible)

**C4: Overlay SpeechRecognizer Context**
`SpeechRecognizer` needs `Activity` context on some Android versions (pre-API 33), but the overlay runs in a `Service` context. Android 14+ also requires `FOREGROUND_SERVICE_MICROPHONE` permission and `android:foregroundServiceType="microphone"` on the service declaration for mic access from foreground services.

*Resolution:* **Overlay voice is deferred to PR #3.** PR #1 and PR #2 implement voice only in `ChatActivity` context (where `RECORD_AUDIO` permission and Activity context are guaranteed). PR #3 adds overlay mic with these prerequisites:
- `FOREGROUND_SERVICE_MICROPHONE` permission in manifest (added in PR #1 proactively)
- `android:foregroundServiceType="microphone"` on `OverlayService` declaration
- Fallback plan: if `SpeechRecognizer` fails with service context, launch transparent `RecordingActivity`
- The bubble walkie-talkie gesture (#576) lives entirely in PR #3

### 3.2 Deferred (File as Issues)

**D1: Cloud STT Providers (OpenAI Whisper, Deepgram, Groq)**
- Record audio to file → upload → get transcript
- Different latency profile than real-time SpeechRecognizer (no partials)
- Need audio recording infrastructure (`MediaRecorder` or `AudioRecord`)
- File issue: "Cloud STT providers: OpenAI Whisper, Deepgram, Groq"

**D2: Cloud TTS Providers (OpenAI, ElevenLabs)**
- HTTP request → receive audio bytes → play via `AudioTrack` or `MediaPlayer`
- Streaming playback for lower latency (ElevenLabs supports chunked streaming)
- Need API key validation flow in settings
- File issue: "Cloud TTS providers: OpenAI TTS, ElevenLabs"

**D3: Bubble 3D Press / Force Touch Voice Activation (v2)**
- Long press on overlay bubble activates voice mode
- Need haptic feedback on activation (`HapticFeedbackConstants.LONG_PRESS`)
- Visual state change: bubble transitions to "listening" mode (pulsing animation)
- Existing `onLongPress` callback on `OverlayBubble` already exists — would need to repurpose or add gesture distinction
- Current long press shows quick action menu — need to decide: replace menu? add gesture variant?
- Android doesn't expose true force/pressure data on most devices; "3D press" = long press with haptic + visual feedback
- File issue: "Overlay bubble long-press voice activation with haptic feedback"

**D4: Auto-Summary for Long TTS (OpenClaw pattern)**
- When response > N chars, summarize before speaking
- Requires a cheap/fast model for summarization
- Can reuse action-tier model from ModelClassifier
- File issue: "Auto-summarize long responses before TTS"

**D5: Wake Word Detection**
- Always-on listening for "Hey Fawx" (or configurable trigger)
- Significant battery/privacy implications
- Original spec mentions Porcupine for wake words
- File issue: "Wake word detection for hands-free activation"

**D6: Voice Activity Detection (VAD)**
- Auto-detect when user stops speaking (vs requiring button release)
- Silero VAD or Android's built-in partial result gaps
- Enables tap-to-toggle mode (tap mic → speak → auto-detect silence → send)
- File issue: "Voice Activity Detection for auto-send"

**D7: Streaming Playback for Cloud TTS**
- ElevenLabs and OpenAI support chunked audio streaming
- Play audio as chunks arrive instead of waiting for full synthesis
- Significantly reduces perceived latency for long responses
- File issue: "Streaming TTS playback for cloud providers"

### 3.3 Intentional Divergences (Documented)

**ID1: No `[[tts:...]]` directive system in v1**
OpenClaw lets the LLM control voice parameters per-reply. We skip this because:
- Fawx v1 has a single agent voice, not multi-persona
- Adding directive parsing adds prompt complexity
- Can revisit if we add personality/character features

**ID2: No channel-aware format negotiation**
OpenClaw picks Opus/MP3 based on delivery channel. Fawx doesn't need this because:
- Output is always local device speaker
- Android handles audio format internally
- If we ever add "share voice reply" (e.g., send audio to messaging), we'd add format negotiation then

**ID3: On-device priority over cloud**
OpenClaw defaults to cloud (OpenAI/ElevenLabs) with Edge TTS as fallback. Fawx inverts this:
- Default = Android built-in (zero latency, zero cost, offline)
- Cloud = opt-in upgrade for quality
- This matches the phone-native philosophy from SPEC.md

---

## 4. Integration Points Analysis

### 4.1 Chat Input Bar

**Current:** `ChatInputBar` has text field + send button. `trailingContent` slot exists but unused.

**Voice addition:**
- Add mic `IconButton` to the left of send button (or swap send↔mic based on input state: empty → mic, has text → send)
- Mic button states: default → listening (animated) → processing
- Partial transcript shown in text field as user speaks
- On final transcript: populate text field, user can edit before sending OR auto-send

**Decision needed:** Tap-to-toggle vs hold-to-talk vs both?
- Tap-to-toggle: tap mic → speak → tap again (or auto-detect silence) → done
- Hold-to-talk: press and hold mic → speak → release → done
- Recommendation: **tap-to-toggle** for v1 (simpler, accessible), add hold-to-talk option later

### 4.2 Overlay Mini-Chat

**Current:** `OverlayMiniChatContent` has a text field + submit chip.

**Voice addition:**
- Mic button next to submit
- Same behavior as main chat
- Extra consideration: overlay may not have focus — `SpeechRecognizer` should still work

### 4.3 Overlay Bubble (v2: 3D Press)

**Current:** `OverlayBubble` has `onClick` (expand) and `onLongClick` (quick action menu).

**v2 addition:**
- Long press → enter voice mode instead of (or in addition to) quick action menu
- Bubble visual transitions to "listening" state (pulsing rings, mic icon overlay)
- Haptic confirmation on activation
- Speak → auto-send → response spoken via TTS
- This creates a full voice loop without ever opening the mini-chat

### 4.4 Settings — Sound & Haptics

**Current:** "Sound & Haptics" entry exists in `SettingsHubScreen` navigation items.

**Voice settings to add:**
- **Voice Input** section: STT provider dropdown, tap-to-toggle vs hold-to-talk
- **Voice Output** section: TTS provider dropdown, speed slider, pitch slider, auto-speak toggle
- **Test** button: "Test voice output" → `activeTts.speak("Hello, I'm Fawx")` (works with PR #1 infrastructure). "Test microphone" (record + playback) deferred to PR #3 (requires `MediaRecorder`).

### 4.5 ChatViewModel Integration

**Current:** `sendMessage(content: String)` is the entry point.

**Voice flow:**
```
Mic tap → VoiceManager.activeStt.startListening()
  → SpeechEvent.Partial(text) → update inputText in real-time
  → SpeechEvent.Final(text) → set inputText
  → autoSendAfterVoice setting?
    → true → sendMessage(text) immediately
    → false (default) → show in text field, user reviews + taps send
  → response arrives → autoSpeakResponses setting?
    → true → VoiceManager.activeTts.speak(response.content)
    → false → show text only
```

No changes needed to `sendMessage()` itself — voice just provides the input text.
`autoSendAfterVoice` defaults to `false` (review first). Users can enable in Settings > Sound & Haptics.
Note: bubble walkie-talkie gesture (PR #3) always auto-sends regardless of this setting.

---

## 5. Test Strategy

### 5.1 Unit Tests (`:core`)

| Test | What it validates |
|------|-------------------|
| `AndroidSpeechRecognizerTest` | State machine transitions, error handling, timeout |
| `AndroidTextToSpeechTest` | Init callback handling, speak queueing, release cleanup |
| `VoiceManagerTest` | Provider registration, switching, persistence, availability checks |
| `SpeechErrorTest` | Error type mapping from Android error codes |

Note: Tests mock the `SpeechToTextProvider` and `TextToSpeechProvider` interfaces directly — no Robolectric needed for provider logic tests. `VoiceManager` tests use mock providers. Only `AndroidSpeechToText`/`AndroidTextToSpeech` implementation tests need Android framework stubs (these can be `@Ignore`d for unit tests and covered in manual testing).

### 5.2 Integration Points (`:chat`)

| Test | What it validates |
|------|-------------------|
| `ChatInputBarVoiceTest` | Mic button visibility, state transitions, partial text display |
| `VoiceSettingsScreenTest` | Provider selection UI, slider controls, test buttons |
| `OverlayVoiceTest` | Mic button in mini-chat, bubble long-press (v2) |

### 5.3 Manual Test Matrix

| Scenario | Expected |
|----------|----------|
| Mic button tap with permission denied | Shows rationale or guides to Settings |
| Speak short phrase | Partial text appears, final text in input field |
| Speak then edit before sending | Text editable, send button active |
| Auto-speak toggle on + agent responds | Response read aloud via TTS |
| Switch STT provider in settings | Next mic tap uses new provider |
| Airplane mode + Android STT | Works (on-device recognition) |
| Airplane mode + cloud STT | SpeechEvent.Error(NetworkError) emitted, error shown to user, must manually switch provider in Settings |
| TTS while agent is still streaming | Waits for stream to complete, then speaks full response |

---

## 6. Implementation Order

### PR #1: Interfaces + Android Implementations (`:core`)
1. `SpeechToTextProvider` interface + `SpeechEvent` sealed class + `SpeechError` sealed class
2. `TextToSpeechProvider` interface + `TtsOptions`
3. `AndroidSpeechToText` implementation (wraps `SpeechRecognizer`, Flow-based)
4. `AndroidTextToSpeech` implementation (wraps `TextToSpeech`)
5. `VoiceManager` coordinator (StateFlow for active providers, Mutex for thread safety)
6. Tests for all of the above (mock interfaces directly — no Robolectric needed for provider tests)
7. Manifest additions:
   - `RECORD_AUDIO` permission
   - `FOREGROUND_SERVICE_MICROPHONE` permission (needed for PR #3 overlay mic)
   - `android:foregroundServiceType="microphone"` on `OverlayService` declaration

### PR #2: UI Integration (`:chat`) — ChatActivity only, no overlay
1. Mic button in `ChatInputBar` (tap-to-toggle)
2. Voice recording state in `ChatViewModel`
3. Partial transcript display (Flow collection → inputText update)
4. Auto-speak responses (final response only, not tool status)
5. Permission request flow in `ChatActivity` (request on first mic tap, pre-check on startup)
6. Voice settings in Sound & Haptics screen (provider selection, speed, auto-speak, auto-send toggle)

### PR #3 (deferred): Overlay Voice + Advanced
1. Mic button in overlay mini-chat (requires service context testing per C4)
2. Bubble walkie-talkie gesture: hold 2s → record → release → send (#576)
3. Cloud provider stubs (OpenAI Whisper, ElevenLabs)

---

## 7. Resolved Decisions

1. **✅ Tap-to-toggle** for chat input bar mic button (v1). Hold-to-talk is the bubble gesture (v2).
2. **✅ Review before send** (default). `autoSendAfterVoice = false` — transcript shown in text field. Configurable in Settings.
3. **✅ Final response only** for TTS. Tool status messages ("Opening app...", "Tapping...") are not spoken.
4. **✅ Bubble walkie-talkie** (v2, #576): Hold 2s → haptic + visual → record while holding → release → auto-send → TTS response. Quick action menu relocates (TBD in PR #3).

## 8. Implementation Notes

- **STT Flow pattern:** Use `callbackFlow { awaitClose { recognizer.stopListening(); recognizer.destroy() } }` to wrap `SpeechRecognizer` callbacks into `Flow<SpeechEvent>`. This is a well-established pattern for Android callback APIs.

## 9. Remaining Open Question

1. **Quick action menu relocation (v2):** Where does the current long-press menu move when bubble becomes walkie-talkie? Options: icon in mini-chat header, triple-tap, swipe up. Resolve during PR #3.
