# Spec: fawx-skill-tts (Text-to-Speech)

**Status:** Draft  
**Date:** 2026-03-08  
**Repo:** `fawxai/fawx-skill-tts`

---

## 1. Problem

Fawx can't produce audio output. For Telegram and future voice interfaces, text-to-speech enables spoken responses.

## 2. Approach

WASM skill that calls a TTS API and returns an audio file path.

## 3. Tool

```json
{
  "name": "text_to_speech",
  "description": "Convert text to speech audio. Returns path to the audio file.",
  "parameters": {
    "text": "string — text to convert (max 4096 chars)",
    "voice": "string — voice ID (default: 'alloy')",
    "model": "string — TTS model (default: 'tts-1')",
    "format": "string — output format: mp3|opus|aac|flac (default: 'opus')"
  }
}
```

## 4. Implementation

### Provider: OpenAI TTS API

```
POST https://api.openai.com/v1/audio/speech
{
  "model": "tts-1",
  "input": "Hello world",
  "voice": "alloy"
}
```

Returns raw audio bytes. Save to `~/.fawx/media/audio/<hash>.opus`.

### Telegram Integration

After generating audio, the Telegram channel can send it as a voice message:
```rust
telegram.send_voice(chat_id, audio_path).await
```

This requires adding `send_voice` to `TelegramChannel` (separate from this skill).

### Config

```toml
[tts]
provider = "openai"  # or "elevenlabs"
default_voice = "alloy"
default_model = "tts-1"
# api_key via credential store
```

## 5. Testing

- Mock API server, verify request format
- Handle text too long (truncate or chunk)
- Handle API error
- Output file written with correct format
- Voice parameter passed correctly
