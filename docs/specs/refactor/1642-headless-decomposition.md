# Spec: #1642 — Decompose headless.rs into Command Handlers

## Status
Not started.

## Goal
Split `engine/crates/fx-cli/src/headless.rs` (5,765 lines) into focused modules organized by command domain.

## Current State (codex/provider-owned-loop-refactor branch)

File: `engine/crates/fx-cli/src/headless.rs` (5,787 lines)

### Content inventory

The file contains two main structs and their implementations:

**`HeadlessEngine` (~1,400 lines of impl)**
Core headless mode engine. Key methods:
- `process_message()`, `process_message_streaming()` (line 1262+)
- `process_message_for_source()`, `process_message_with_attachments()` (1277+)
- `process_message_with_images()`, `process_message_with_context()` (1298+)
- `process_message_for_source_streaming()` (1333)
- `handle_thinking()` (1427)

**`HeadlessSession` (~2,500 lines of impl)**
Interactive headless session that wraps HeadlessEngine. Key methods:
- `process_input()` (line 1688) — main input router
- `process_message()` (2038)
- `process_message_with_context()` (2068)
- `handle_thinking()` (2266)
- `handle_synthesis()` (2292)
- `handle_auth()` (2296)
- `handle_keys()` (2308)
- `handle_sign()` (2319)

**Free functions — command handlers (~1,800 lines)**
- `handle_headless_synthesis_command()` (line 2360)
- `handle_headless_auth_command()` (2395)
- `handle_headless_keys_command()` (2622)
- Auth subcommand matching (line 2405): 20+ arm match on `(subcommand, action, value, has_extra_args)`

### Command domains identified

1. **Message processing** — `process_message*` variants, streaming, attachments, images
2. **Auth management** — `handle_auth`, `handle_headless_auth_command`, setup token, OAuth, API key management
3. **Key management** — `handle_keys`, `handle_headless_keys_command`, signing
4. **Model/thinking config** — `handle_thinking`, `handle_synthesis`, model switching
5. **Input routing** — `process_input`, slash command parsing, dispatch

## Proposed Decomposition

```
fx-cli/src/headless/
├── mod.rs            (~200 lines — re-exports, HeadlessEngine + HeadlessSession struct defs)
├── engine.rs         (~800 lines — HeadlessEngine message processing impl)
├── session.rs        (~600 lines — HeadlessSession core: process_input, routing)
├── auth.rs           (~800 lines — auth commands, setup token, OAuth, API keys)
├── keys.rs           (~400 lines — key management, signing)
├── model.rs          (~300 lines — thinking level, synthesis instruction, model config)
├── message.rs        (~600 lines — message processing variants, streaming, attachments)
```

## Deliverables

1. Convert `headless.rs` file into `headless/` module directory
2. Move impl blocks and free functions into domain modules
3. HeadlessEngine and HeadlessSession struct definitions stay in `mod.rs`
4. Each domain module contains `impl HeadlessEngine` and/or `impl HeadlessSession` blocks for its domain
5. All public API unchanged — `use crate::headless::HeadlessEngine` still works
6. All existing tests pass. Tests can stay in a `tests.rs` submodule or at the bottom of their domain module.
7. No behavioral changes

## Files to modify
- `engine/crates/fx-cli/src/headless.rs` → `engine/crates/fx-cli/src/headless/mod.rs` + submodules
- Update any `mod headless;` declaration in `engine/crates/fx-cli/src/lib.rs` or `main.rs` (should work automatically with directory module)

## Not in scope
- Refactoring the auth command match arms (the 20+ arm match stays, just moves to auth.rs)
- Adding new commands
- Changing HeadlessEngine/HeadlessSession interfaces
- Trait-based command dispatch (that would be a separate follow-up)
