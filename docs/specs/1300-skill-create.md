# Spec: `fawx skill create` scaffolding command (#1300)

## Summary

Add `fawx skill create <name>` to scaffold a new WASM skill project with all boilerplate needed to build, sign, and install a skill.

## Files to touch

- `engine/crates/fx-cli/src/commands/skills.rs` — add `create()` function
- `engine/crates/fx-cli/src/commands/mod.rs` — wire `skill create` subcommand (if not already routed)
- `engine/crates/fx-cli/src/main.rs` or equivalent CLI entry — ensure `skill create` is dispatched

## Behavior

### Command

```bash
fawx skill create <name> [--capabilities <cap1,cap2>] [--tool-name <name>] [--path <dir>]
```

### Arguments

| Arg | Required | Default | Description |
|-----|----------|---------|-------------|
| `name` | yes | — | Skill name (validated: no path separators, no `..`, max 64 chars, alphanumeric + hyphens) |
| `--capabilities` | no | `[]` | Comma-separated capabilities to pre-fill in manifest (`network`, `storage`, `notifications`, `sensors`, `phone_actions`) |
| `--tool-name` | no | same as `name` | Primary tool name in the manifest |
| `--path` | no | `./skills/<name>` | Directory to create the project in |

### Generated files

All generated under `<path>/<name>/`:

#### `Cargo.toml`
```toml
[package]
name = "<name>"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
# No deps by default — host API is provided via imports
```

#### `manifest.toml`
```toml
name = "<name>"
version = "0.1.0"
description = "A Fawx skill"
author = ""
api_version = "host_api_v2"
entry_point = "run"
capabilities = [<from --capabilities flag>]

[[tools]]
name = "<tool-name>"
description = "TODO: describe what this tool does"

[[tools.parameters]]
name = "input"
type = "string"
description = "TODO: describe the input parameter"
required = true
```

#### `src/lib.rs`
```rust
//! <name> — a Fawx WASM skill.

/// Entry point called by the Fawx host.
///
/// The host provides input as a JSON string via the `input` parameter.
/// Return a JSON string as the tool result.
#[no_mangle]
pub extern "C" fn run(input_ptr: *const u8, input_len: usize) -> u64 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(input_ptr, input_len);
        std::str::from_utf8_unchecked(slice)
    };

    // TODO: implement your skill logic here
    let result = format!("{{\"result\": \"Hello from {name}! Input was: {{}}\"}}", input);

    let bytes = result.into_bytes();
    let ptr = bytes.as_ptr() as u64;
    let len = bytes.len() as u64;
    std::mem::forget(bytes);

    (ptr << 32) | len
}
```

#### `.gitignore`
```
/target
```

#### `README.md`
```markdown
# <name>

A Fawx WASM skill.

## Build

\`\`\`bash
cargo build --release --target wasm32-unknown-unknown
\`\`\`

## Install

\`\`\`bash
fawx skill install target/wasm32-unknown-unknown/release/<name>.wasm
\`\`\`
```

### Validation

- Name must pass the same validation as `validate_manifest()` in `fx-skills/src/manifest.rs`: no empty, no path separators, no `..`, valid length
- If `--capabilities` includes unknown values, error with list of valid capabilities
- If target directory already exists, error with clear message (no silent overwrite)

### Output

On success, print:
```
Created skill project: <path>/<name>/

To build:
  cd <path>/<name>
  cargo build --release --target wasm32-unknown-unknown

To install:
  fawx skill install target/wasm32-unknown-unknown/release/<name_underscored>.wasm
```

### Error cases

| Condition | Behavior |
|-----------|----------|
| Invalid name | Error with reason |
| Directory exists | Error: "directory already exists: <path>" |
| Unknown capability | Error: "unknown capability '<cap>', valid: network, storage, notifications, sensors, phone_actions" |
| Can't create directory | Error with OS error |

## Testing

### Unit tests (in `skills.rs`)

1. `create_scaffolds_all_files` — create with defaults, assert all 5 files exist with expected content
2. `create_with_capabilities` — `--capabilities network,storage`, assert manifest contains both
3. `create_with_custom_tool_name` — `--tool-name my_tool`, assert manifest uses it
4. `create_with_custom_path` — `--path /tmp/test-skills`, assert files created there
5. `create_rejects_invalid_name` — `../evil`, `foo/bar`, empty string, 65-char name
6. `create_rejects_existing_directory` — pre-create dir, assert error
7. `create_rejects_unknown_capability` — `--capabilities flying`, assert error with valid list
8. `create_manifest_parses_cleanly` — generated manifest passes `parse_manifest()` + `validate_manifest()`

## Complexity estimate

~200 lines of new code + ~150 lines of tests. Single file change plus CLI wiring. Straightforward scaffolding — no async, no network, no state.
