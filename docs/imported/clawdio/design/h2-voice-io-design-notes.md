# H2 Voice I/O Design Notes

*Created 2026-02-17. Joe wants voice in Fawx MVP v2.*

## Decision

Voice I/O as an input/output layer around the existing text-based AgentExecutor. Not a new execution model — just a different way to get text in and audio out.

## MVP (V1): All On-Device, Zero Dependencies

- **STT**: Android `SpeechRecognizer` API (on-device, free, no API key)
- **TTS**: Android `TextToSpeech` API (on-device, free, no API key)
- **Latency**: ~600-800ms first audio byte
- **Effort**: Low — both are standard Android APIs
- **Voice quality**: Functional, not impressive. Good enough to validate the UX.

### Architecture

```
Mic → SpeechRecognizer → text → AgentExecutor (existing) → response text → TextToSpeech → Speaker
```

- AgentExecutor doesn't change at all
- Voice is a UI-layer concern (lives in `:chat` module, Jarvis territory)
- Streaming: TTS can start speaking first sentence while LLM generates rest
- Tool calls work normally — user hears "Searching the web..." or similar status

### Key Design Points

- **Activation**: Hold-to-talk button or wake word (hold-to-talk for MVP)
- **During tool execution**: Play short status audio ("Looking that up...") or silence
- **Interruption**: User taps again → cancel TTS playback, start new STT
- **Fallback**: If SpeechRecognizer fails, show keyboard input (already exists)
- **No new permissions**: `RECORD_AUDIO` is the only addition

## V2: Better TTS Provider

- Replace Android TTS with streaming cloud TTS (ElevenLabs, Cartesia Sonic, etc.)
- User provides API key (fits wallet/key model)
- Latency: ~400-600ms first audio byte
- Voice quality: Dramatic improvement

## V3: OpenAI Realtime API (Premium Mode)

- Native audio-in, audio-out via OpenAI
- ~200-300ms latency
- Requires OpenAI key
- Supports function calling (tools work)
- Separate provider type, not a wrapper around AgentExecutor

## V4: Self-Hosted Hybrid (Future/Separate Product)

- PersonaPlex-7B (conversation) + Qwen3-TTS (synthesis)
- Requires GPU server infrastructure
- Joe has research from Grok conversation (Feb 2026)
- Models: PersonaPlex-7B (~14GB quantized), Qwen3-TTS-12Hz-1.7B-CustomVoice (~3.4GB)
- Repos to watch: QwenLM/Qwen3-TTS, NVIDIA/personaplex
- No polished public hybrid exists yet as of Feb 2026

## Why Not Speech-to-Speech Models for MVP?

- PersonaPlex needs GPU server (7B model, ~14GB) — violates "phone IS the stack"
- Locks you into one model for reasoning (can't use Claude/GPT)
- Tools don't work through speech-to-speech models
- The hybrid adds ~300-500ms latency from chaining two models
- Building/hosting that infra is a separate product, not a Fawx feature

## Fawx Integration Points

- **`:chat` module**: Voice button UI, SpeechRecognizer/TextToSpeech lifecycle
- **`ChatViewModel`**: Voice state management (listening, processing, speaking)
- **`AgentExecutor`**: No changes needed — voice is text by the time it reaches here
- **`PhoneTools`**: No changes
- **Streaming (`onTextDelta`)**: Hook TTS streaming here for sentence-level synthesis

## File Ownership

Voice UI components → Jarvis (`:chat` module, UI layer)
Voice provider abstraction (V2+) → Clawdio (if we add a `VoiceProvider` interface in `:core`)

## References

- Joe's Grok conversation on PersonaPlex + Qwen3-TTS hybrid (Feb 16, 2026)
- Grok's production spec for hybrid deployment
- GitHub repos: QwenLM/Qwen3-TTS (~7.8k stars), NVIDIA/personaplex, jamiepine/voicebox
- HuggingFace: Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice
