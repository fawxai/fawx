# Fawx Skills

Example WASM skills for the Fawx AI agent.

## Available Skills

### Weather Skill
Fetches weather information for a given location.

**Capabilities:** `network`, `storage`

**Input format:**
```json
{
  "location": "San Francisco"
}
```

**Output format:**
```json
{
  "location": "San Francisco",
  "temperature": 22.5,
  "condition": "Sunny"
}
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

## Building Skills

### Prerequisites
- Rust toolchain with `wasm32-wasi` target

```bash
rustup target add wasm32-wasi
```

### Build All Skills
```bash
cd skills
./build.sh
```

### Build Individual Skills
```bash
cd weather-skill
cargo build --target wasm32-wasi --release
```

The compiled WASM binary will be at `target/wasm32-wasi/release/weather_skill.wasm`.

## Installing Skills

Use the Fawx CLI to install skills:

```bash
fawx skill install skills/weather-skill/weather.wasm
fawx skill install skills/calculator-skill/calculator.wasm
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

- Skills must be compiled for `wasm32-wasi` target
- Skills run in a sandboxed environment
- Only declared capabilities are granted
- Skills communicate via JSON input/output
- Skills should handle errors gracefully
