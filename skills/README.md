# Fawx Skills

Example WASM skills for the Fawx AI agent.

## Available Skills

### Weather Skill
Fetches current weather conditions and a 3-day forecast for a given location.

**Capabilities:** `network`

**Input format:**
```json
{
  "location": "San Francisco",
  "units": "fahrenheit"
}
```

**Output format:**
```text
🌤️ Weather for San Francisco

Current: 68°F (20°C), Partly Cloudy
Humidity: 71% | Wind: 9 mph

📅 3-Day Forecast:
  Mon: ☀️ 70°F / 55°F — Clear Sky
  Tue: 🌧️ 63°F / 52°F — Rain
  Wed: ⛅ 66°F / 53°F — Partly Cloudy
```

### Calculator Skill
Evaluates simple mathematical expressions.

**Capabilities:** None

**Input format:**
```json
{
  "expression": "2 + 3 * 4"
}
```

**Output format:**
```json
{
  "result": 14.0,
  "expression": "2 + 3 * 4"
}
```

### Vision Skill
Analyzes images with vision-capable LLMs using Anthropic Claude or OpenAI GPT-4o.

**Capabilities:** `network`, `storage`

**Input format:**
```json
{
  "image": "https://example.com/cat.png",
  "prompt": "Describe this image in detail",
  "provider": "anthropic"
}
```

**Output format:**
```text
🔍 Image Analysis (Claude):

A black cat sitting on a couch and looking at the camera.
```

### TTS Skill
Converts text to speech with OpenAI TTS and returns base64-encoded MP3 audio.

**Capabilities:** `network`, `storage`

**Input format:**
```json
{
  "text": "Hello from Fawx",
  "voice": "alloy",
  "provider": "openai",
  "speed": "1.0"
}
```

**Output format:**
```json
{
  "status": "success",
  "provider": "openai",
  "voice": "alloy",
  "format": "mp3",
  "audio_base64": "<base64 encoded mp3>",
  "text_length": 15,
  "message": "🔊 Generated speech (15 chars, voice: alloy, OpenAI TTS)"
}
```

### Browser Skill
Fetches web pages, extracts readable content, searches the web with Brave Search, and can return screenshots via a configured screenshot service.

**Capabilities:** `network`, `storage`

**Input format:**
```json
{
  "tool": "web_fetch",
  "url": "https://example.com",
  "format": "markdown",
  "max_length": "10000"
}
```

```json
{
  "tool": "web_search",
  "query": "rust async programming",
  "count": "5"
}
```

```json
{
  "tool": "web_screenshot",
  "url": "https://example.com",
  "width": "1280",
  "height": "720"
}
```

**Output format:**
```json
{
  "status": "success",
  "query": "rust async programming",
  "count": 5,
  "results": [
    {
      "title": "Rust Programming Language",
      "url": "https://www.rust-lang.org",
      "snippet": "A language empowering everyone to build reliable software."
    }
  ],
  "message": "🔍 Found 5 results for: rust async programming"
}
```

## Building Skills

### Prerequisites
- Rust toolchain with `wasm32-unknown-unknown` target

```bash
rustup target add wasm32-unknown-unknown
```

### Build All Skills
```bash
cd skills
./build.sh
```

### Build Individual Skills
```bash
cd weather-skill
cargo build --target wasm32-unknown-unknown --release
```

The compiled WASM binary will be at `target/wasm32-unknown-unknown/release/weather_skill.wasm` (or `browser_skill.wasm` for the browser skill).

## Installing Skills

Use the Fawx CLI to install skills:

```bash
fawx skill install skills/weather-skill/weather.wasm
fawx skill install skills/calculator-skill/calculator.wasm
fawx skill install skills/vision-skill/vision.wasm
fawx skill install skills/tts-skill/tts.wasm
fawx skill install skills/browser-skill/browser.wasm
```

## Manifest Format

Each skill requires a `manifest.toml` file:

```toml
name = "skill-name"
version = "1.0.0"
description = "What the skill does"
author = "Your Name"
api_version = "host_api_v1"
capabilities = ["network", "storage"]
entry_point = "run"
```

### Capabilities

- `network` - Make HTTP requests
- `storage` - Persistent key-value storage
- `notifications` - Send notifications
- `sensors` - Read sensor data
- `phone_actions` - Control phone functions

## Developing Custom Skills

### 1. Create a new Rust library project

```bash
cargo new --lib my-skill
cd my-skill
```

### 2. Update Cargo.toml

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### 3. Implement the `run()` entry point

```rust
#[no_mangle]
pub extern "C" fn run() {
    // Your skill logic here
}
```

### 4. Use host API functions

```rust
extern "C" {
    fn host_api_v1_log(level: u32, msg_ptr: *const u8, msg_len: u32);
    fn host_api_v1_get_input() -> u32;
    fn host_api_v1_set_output(text_ptr: *const u8, text_len: u32);
    fn host_api_v1_kv_get(key_ptr: *const u8, key_len: u32) -> u32;
    fn host_api_v1_kv_set(
        key_ptr: *const u8,
        key_len: u32,
        val_ptr: *const u8,
        val_len: u32,
    );
}
```

### 5. Build for WASM

```bash
cargo build --target wasm32-wasi --release
```

### 6. Create manifest.toml

See format above.

### 7. Install and test

```bash
fawx skill install target/wasm32-wasi/release/my_skill.wasm
fawx skill list
```

## Host API Reference

### Logging

```rust
fn host_api_v1_log(level: u32, msg_ptr: *const u8, msg_len: u32)
```

Levels: 0=trace, 1=debug, 2=info, 3=warn, 4=error

### Input/Output

```rust
fn host_api_v1_get_input() -> u32  // Returns pointer to input string
fn host_api_v1_set_output(text_ptr: *const u8, text_len: u32)
```

### Key-Value Storage

```rust
fn host_api_v1_kv_get(key_ptr: *const u8, key_len: u32) -> u32  // Returns pointer or 0
fn host_api_v1_kv_set(
    key_ptr: *const u8,
    key_len: u32,
    val_ptr: *const u8,
    val_len: u32,
)
```

## Notes

- Skills must be compiled for `wasm32-unknown-unknown` target
- Skills run in a sandboxed environment
- Only declared capabilities are granted
- Skills communicate via JSON input/output
- Skills should handle errors gracefully
