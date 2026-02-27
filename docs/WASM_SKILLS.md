# WASM Skills System

Fawx's WASM skills system provides a secure, sandboxed environment for extending agent capabilities through WebAssembly modules.

## Architecture

### Components

1. **Skill Manifest** (`manifest.toml`)
   - Metadata: name, version, description, author
   - API version contract (`host_api_v1`)
   - Required capabilities
   - Entry point function name

2. **WASM Module** (`skill.wasm`)
   - Compiled from Rust (or other WASM-compatible languages)
   - Exports entry point function (default: `run`)
   - Imports host API functions

3. **Host API** (`host_api_v1`)
   - `log(level, message)` - Logging
   - `get_input()` - Get JSON input
   - `set_output(text)` - Set JSON output
   - `kv_get(key)` - Read from key-value storage
   - `kv_set(key, value)` - Write to key-value storage

4. **Runtime**
   - Loads and validates WASM modules
   - Links host functions via wasmtime
   - Enforces capability permissions
   - Manages execution lifecycle

5. **Registry**
   - Discovers installed skills from `~/.fawx/skills/`
   - Loads skills on demand
   - Provides metadata for agent planning

6. **Cache**
   - Caches compiled modules at `~/.fawx/cache/skills/`
   - Invalidates on WASM file hash change
   - Speeds up repeated skill loads

## Capabilities

Skills declare required capabilities in their manifest:

- `network` - Make HTTP requests
- `storage` - Persistent key-value storage
- `notifications` - Send user notifications
- `sensors` - Read sensor data (location, accelerometer, etc.)
- `phone_actions` - Control phone functions (high privilege)

Capabilities are enforced at runtime. Skills cannot access resources they haven't declared.

## Skill Lifecycle

### Development

1. Write skill in Rust (or other WASM language)
2. Implement `run()` entry point
3. Use host API functions via `extern "C"` imports
4. Compile to `wasm32-wasi` target
5. Create `manifest.toml`

### Installation

```bash
fawx skill install path/to/skill-directory
# or
fawx skill install path/to/skill.wasm
```

Installation:
- Validates manifest
- Verifies WASM module compiles
- Copies to `~/.fawx/skills/{skill-name}/`
- Optionally verifies signature

### Discovery

The agent automatically discovers installed skills at startup:

1. Registry scans `~/.fawx/skills/`
2. Loads all manifests
3. Converts skills to Claude tool definitions
4. Includes in planning context

### Execution

When the agent invokes a skill:

1. Runtime loads WASM module (from cache if available)
2. Creates host state with input
3. Links host API functions
4. Instantiates module
5. Calls entry point
6. Extracts output
7. Returns result to agent

## Example: Calculator Skill

### manifest.toml

```toml
name = "calculator"
version = "1.0.0"
description = "Evaluates mathematical expressions"
author = "Fawx Team"
api_version = "host_api_v1"
capabilities = []
entry_point = "run"
```

### src/lib.rs

```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    expression: String,
}

#[derive(Serialize)]
struct Output {
    result: f64,
}

extern "C" {
    fn host_api_v1_log(level: u32, msg_ptr: *const u8, msg_len: u32);
    fn host_api_v1_get_input() -> u32;
    fn host_api_v1_set_output(text_ptr: *const u8, text_len: u32);
}

fn log(level: u32, msg: &str) {
    unsafe {
        host_api_v1_log(level, msg.as_ptr(), msg.len() as u32);
    }
}

fn get_input() -> String {
    // Read from host memory...
}

fn set_output(text: &str) {
    unsafe {
        host_api_v1_set_output(text.as_ptr(), text.len() as u32);
    }
}

#[no_mangle]
pub extern "C" fn run() {
    log(2, "Calculator skill starting");
    
    let input_json = get_input();
    let input: Input = serde_json::from_str(&input_json).unwrap();
    
    let result = evaluate(&input.expression);
    
    let output = Output { result };
    let output_json = serde_json::to_string(&output).unwrap();
    
    set_output(&output_json);
}
```

### Build

```bash
cargo build --target wasm32-wasi --release
```

### Install

```bash
fawx skill install target/wasm32-wasi/release/calculator_skill.wasm
```

### Use

The agent can now invoke the calculator skill:

```json
{
  "tool": "skill_calculator",
  "input": {
    "input": "{\"expression\": \"2 + 3 * 4\"}"
  }
}
```

Result:
```json
{
  "result": 14.0
}
```

## Security Model

### Sandboxing

- Skills run in WASM sandbox
- No direct file system access
- No direct network access
- No system calls (except through host API)

### Capabilities

- Skills declare required capabilities upfront
- User can review before installation
- Runtime enforces capability constraints
- Denied operations fail gracefully

### Signatures (Optional)

- Skills can be signed with Ed25519
- Loader verifies signatures against trusted keys
- Unsigned skills can be installed with user consent

### Resource Limits

- Memory limits enforced by WASM runtime
- CPU time limits (future)
- Storage quotas per skill (future)
- Network rate limiting (future)

## Agent Integration

### Tool Discovery

When planning, the agent has access to all installed skills as tools. Each skill appears as:

```json
{
  "name": "skill_calculator",
  "description": "Evaluates mathematical expressions",
  "input_schema": {
    "type": "object",
    "properties": {
      "input": {
        "type": "string",
        "description": "JSON input for the skill"
      }
    }
  }
}
```

### Planning Context

The agent's system prompt includes:

```
Available Skills:
- calculator: Evaluates mathematical expressions
- weather: Fetches weather information for a location (requires: network, storage)
```

### Execution

When the agent decides to use a skill:

1. Agent generates tool use with skill input
2. Runtime invokes skill with input
3. Skill executes and produces output
4. Output returned to agent
5. Agent incorporates result in response

## CLI Commands

### List Skills

```bash
fawx skill list
```

Output:
```
Installed skills:

  calculator v1.0.0
    Evaluates mathematical expressions

  weather v1.0.0
    Fetches weather information for a location
    Capabilities: network, storage
```

### Install Skill

```bash
fawx skill install skills/calculator-skill
fawx skill install skills/calculator-skill/calculator.wasm
```

### Remove Skill

```bash
fawx skill remove calculator
```

## Performance

### Module Caching

- Compiled modules cached at `~/.fawx/cache/skills/`
- Cache keyed by WASM file SHA-256 hash
- First load: ~100ms compile time
- Cached load: ~1ms deserialize time
- ~100x speedup for repeated loads

### Execution Overhead

- Skill invocation: ~1-5ms overhead
- Host function calls: ~0.1-0.5µs each
- Memory allocation: handled by WASM runtime
- Negligible impact on agent latency

## Troubleshooting

### Skill Won't Load

- Check manifest is valid TOML
- Verify API version is `host_api_v1`
- Ensure WASM file is present
- Check compilation target is `wasm32-wasi`

### Runtime Errors

- Check skill logs via `log()` host function
- Verify input JSON format matches skill expectations
- Ensure required capabilities are declared
- Check for memory access errors (bounds checking)

### Performance Issues

- Clear cache: `rm -rf ~/.fawx/cache/skills/`
- Check skill doesn't have infinite loops
- Profile skill execution time
- Consider optimizing WASM binary size

## Future Enhancements

- [ ] HTTP host function for network capability
- [ ] Async skill execution
- [ ] Skill dependencies (skill can call other skills)
- [ ] Hot reload during development
- [ ] Skill marketplace
- [ ] Remote skill installation
- [ ] Skill versioning and updates
- [ ] Resource usage metrics
- [ ] Sandboxing levels (strict/permissive)
- [ ] Multi-language SDKs (Python, JavaScript, Go)

## References

- [WebAssembly](https://webassembly.org/)
- [Wasmtime](https://wasmtime.dev/)
- [WASI](https://wasi.dev/)
- [Rust WASM Book](https://rustwasm.github.io/docs/book/)
