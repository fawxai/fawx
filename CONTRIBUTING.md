# Contributing to Fawx

## Code Standards

All contributions must follow [ENGINEERING.md](ENGINEERING.md). Key rules:

- **TDD** — Write the test first. Every bug fix requires a regression test.
- **No slop** — No `.unwrap()` outside tests. No functions > 40 lines. No > 5 params without a struct.
- **DRY** — No copy-paste. Extract and parameterize.
- **Fail fast** — Errors are explicit. No silent catches, no swallowed exceptions.
- **Clippy clean** — `cargo clippy -- -D warnings` with zero warnings.
- **Formatted** — `cargo fmt --all` before every commit.

## Branch Model

```
feature/* → dev → staging → main
```

- **feature/*** — All work happens here. Branch from `dev`.
- **dev** — Integration branch. PRs target `dev`.
- **staging** — Pre-release. Promoted from `dev` after integration testing.
- **main** — Production releases only.

## Pull Requests

1. Branch from `dev`: `git checkout -b feature/my-change origin/dev`
2. Write tests, implement, verify: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
3. Open PR against `dev`
4. Address all review feedback — blocking, non-blocking, and nice-to-have items all get fixed.

### PR Description
- What changed and why
- Test coverage added
- Any new dependencies (with justification per ENGINEERING.md §0)

## WASM Skill Development

Skills are standalone Rust crates compiled to `wasm32-unknown-unknown`. See [docs/WASM_SKILLS.md](docs/WASM_SKILLS.md) and existing skills in `skills/` for examples.

### Quick Start

```bash
# Create a new skill
mkdir skills/my-skill && cd skills/my-skill
cargo init --lib

# Set crate type in Cargo.toml
# crate-type = ["cdylib"]

# Implement the `run()` export using host_api_v1
# See skills/weather-skill/src/lib.rs for a minimal example

# Build
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown

# Test
cargo test

# Install
fawx skill install target/wasm32-unknown-unknown/release/my_skill.wasm
```

### Manifest

Every skill needs a `manifest.toml`:

```toml
name = "my_skill"
version = "1.0.0"
description = "What the skill does"
author = "Your Name"
api_version = "host_api_v1"
capabilities = ["network"]  # network, storage, or both
entry_point = "run"

[[tools]]
name = "my_tool"
description = "What the tool does"

[[tools.parameters]]
name = "input"
type = "string"
description = "The input parameter"
required = true
```

## Testing

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p fx-tools

# Skill tests
cd skills/weather-skill && cargo test

# Full CI check
cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full crate map and design decisions. Key principle: the kernel is immutable at runtime. If your change touches `fx-kernel`, `fx-auth/src/crypto/`, or `.github/`, it will be blocked by the proposal gate.
