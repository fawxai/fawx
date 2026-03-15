# Spec: CLI Output Polish (V2)

**Phase:** 3c (Polish)  
**Status:** Draft  
**Author:** Clawdio  
**Date:** 2026-03-10

---

## Problem

Current CLI command outputs (`fawx doctor`, `fawx status`, `fawx version`, `fawx update`, `fawx security-audit`) use plain `println!` with no color, no structure, and no visual hierarchy. They're functional but feel bare compared to polished CLI tools.

## Inspiration

OpenClaw's CLI output style: colored section headers, emoji prefixes, dimmed secondary text, dashed separators, diamond bullet points. It makes the terminal feel alive and information easy to scan.

## Solution

Add a shared output formatting module that all CLI commands use. Consistent colors, emojis, and structure across every command.

## Color Palette

| Element | Color | ANSI |
|---------|-------|------|
| Brand/header | Orange/amber | `\x1b[38;5;208m` or `\x1b[33m` (yellow fallback) |
| Success | Green | `\x1b[32m` |
| Warning | Yellow | `\x1b[33m` |
| Error | Red | `\x1b[31m` |
| Dim/secondary | Gray | `\x1b[2m` (dim) |
| Bold text | Bold | `\x1b[1m` |
| Reset | Reset | `\x1b[0m` |

Use 256-color (orange 208) when `$TERM` supports it, fall back to basic ANSI yellow otherwise. Respect `NO_COLOR` environment variable (no color output when set).

## Command Output Designs

### `fawx version`

```
🦊 Fawx 0.1.0 (a8494529)
   Built 2026-03-10 · rustc 1.85.0 · aarch64-apple-darwin
```

### `fawx status`

```
🦊 Fawx Status

◆ Engine ─────────────────────────────
  PID:        42301
  Uptime:     2h 14m
  Port:       8400
  Provider:   anthropic (claude-sonnet-4-20250514)

◆ Memory ─────────────────────────────
  Sessions:   3 active
  Entries:    247 stored
  Index:      1,024 embeddings

◆ Skills ─────────────────────────────
  Loaded:     6/8
  ✓ weather  ✓ vision  ✓ tts
  ✓ browser  ✓ canvas  ✓ stt
  ✗ scheduler (signature invalid)
  ✗ calculator (load error)
```

### `fawx doctor`

```
🦊 Fawx Doctor

◆ System ─────────────────────────────
  ✓ Rust toolchain    1.85.0
  ✓ WASM target       wasm32-unknown-unknown
  ✓ Data directory    ~/.fawx/
  ✓ Config file       ~/.fawx/config.toml

◆ Providers ──────────────────────────
  ✓ Anthropic         connected (claude-sonnet-4-20250514)
  ✗ OpenAI            no credentials configured
  ○ Local             not configured

◆ Credentials ────────────────────────
  ✓ Credential store  encrypted, 2 entries
  ✓ Permissions       600 (owner-only)

◆ Skills ─────────────────────────────
  ✓ 6 skills loaded
  ✓ All signatures valid
  ⚠ 2 skills unsigned (weather, calculator)

◆ Embedding Model ────────────────────
  ✓ Model loaded      nomic-embed-text-v1.5
  ✓ Integrity check   passed

  All checks passed.
```

### `fawx update dev`

```
🦊 Fawx Update

◆ Pre-flight ─────────────────────────
  ✓ Repository        /Users/joseph/fawx
  ✓ Working tree      clean
  ✓ Toolchain         cargo 1.85.0, wasm32 target

◆ Git ────────────────────────────────
  ✓ Fetched origin
  ✓ Pulled dev        a8494529..f1c23b47 (3 commits)

◆ Build ──────────────────────────────
  ✓ Engine            built (38s)
  ✓ TUI               built (15s)
  ✓ Skills            8 built, 8 installed

◆ Restart ────────────────────────────
  ✓ Stopped           pid 42301
  ✓ Started           pid 42589, port 8400

  Update complete. ✓
```

### `fawx security-audit`

```
🦊 Fawx Security Audit

◆ Credential Store ───────────────────
  ✓ Encryption        AES-256-GCM
  ✓ File permissions  600 (owner-only)
  ✓ No plaintext keys in config

◆ WASM Signatures ────────────────────
  ✓ weather.wasm      signed (ed25519)
  ✓ vision.wasm       signed (ed25519)
  ⚠ calculator.wasm   unsigned
  ✓ Trusted keys      2 loaded

◆ Tier 3 Integrity ───────────────────
  ✓ Kernel hash       matches baseline
  ✓ Auth crypto       matches baseline
  ✓ CI configs        matches baseline

◆ Log Scan ───────────────────────────
  ✓ No credentials found in logs
  ✓ Scanned 3 log files (12,847 lines)

  Audit passed. 1 warning (unsigned skill).
```

## Implementation

### New module: `engine/crates/fx-cli/src/output.rs`

```rust
/// Shared CLI output formatting.
///
/// Respects `NO_COLOR` env var and non-TTY stdout.

pub struct Printer {
    color_enabled: bool,
}

impl Printer {
    pub fn new() -> Self { ... }  // detect TTY + NO_COLOR
    
    pub fn header(&self, emoji: &str, title: &str) { ... }
    pub fn section(&self, title: &str) { ... }
    pub fn success(&self, label: &str, value: &str) { ... }
    pub fn warning(&self, label: &str, value: &str) { ... }
    pub fn error(&self, label: &str, value: &str) { ... }
    pub fn dim(&self, text: &str) { ... }
    pub fn separator(&self) { ... }
    pub fn footer(&self, message: &str) { ... }
}
```

### Integration

Each command gets updated to use `Printer` instead of raw `println!`. The `Printer` struct handles all ANSI escaping, padding, and alignment.

### `NO_COLOR` and pipe detection

- If `NO_COLOR` env var is set (any value), disable all colors and emojis
- If stdout is not a TTY (piped to file or another command), disable colors but keep structure
- Use `atty::is(Stream::Stdout)` or `std::io::stdout().is_terminal()` (Rust 1.70+)

### Dependencies

- No new crates. Use `std::io::IsTerminal` (stable since 1.70) for TTY detection.
- ANSI escape codes are simple string constants, no library needed.

## Testing

1. **Printer disables color when NO_COLOR is set**
2. **Printer disables color when stdout is not TTY**
3. **Success/warning/error produce correct prefixes**
4. **Section headers have correct formatting**
5. **Output is valid UTF-8 with correct escape sequences**

## Rollout

Apply to commands in this order (each can be a separate commit):
1. `output.rs` module + `fawx version` (smallest, proves the pattern)
2. `fawx doctor`
3. `fawx status`
4. `fawx update` + `fawx restart`
5. `fawx security-audit`
6. `fawx logs` (header only)

## Not in scope
- Progress bars or spinners (future, for long builds)
- Interactive/TUI-style output in CLI commands
- Color configuration in config.toml
