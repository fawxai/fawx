# Spec: Setup Wizard — Skill Selection + Credential Flow

**Phase:** 3b (Ops)  
**Status:** Draft  
**Depends on:** feat/skill-credentials PR  
**Author:** Clawdio  
**Date:** 2026-03-10

---

## Overview

Extend `fawx setup` to include skill installation and credential configuration as part of the first-run wizard. Users shouldn't need to know about `skills/build.sh --install` or `fawx auth set-credential`.

---

## Flow

```
fawx setup

◆ Step 1: LLM Provider
  Select a provider:
    1. Anthropic (Claude)
    2. OpenAI (GPT)
    3. Both
  > 1

  Anthropic API key: sk-ant-...
  Testing connection... ✓ claude-sonnet-4-6

  Select default model:
    1. claude-opus-4-6
    2. claude-sonnet-4-6
    3. claude-haiku-4-5
  > 2

◆ Step 2: Skills
  Install recommended skills? (arrow keys to toggle, enter to confirm)

  [x] Calculator       free, no key needed
  [x] Weather          free, no key needed
  [x] Canvas           free, no key needed
  [ ] TTS              uses your OpenAI key ← dimmed if no OpenAI
  [ ] STT              uses your OpenAI key
  [ ] Vision           uses your OpenAI key
  [ ] Browser          needs Brave API key
  [ ] GitHub           needs GitHub PAT

  Installing 3 skills... ✓

◆ Step 3: Skill Credentials (only if needed)
  ── shown only when selected skills require keys not already stored ──

  GitHub Personal Access Token (optional): ghp_...
  ✓ Stored securely

  Brave API key (optional): ...
  ✓ Stored securely

  TTS/STT will use your OpenAI key automatically. ← shown if OpenAI configured

◆ Step 4: Telegram (optional)
  Connect a Telegram bot? (y/n): n

◆ Setup complete!
  Start chatting:
    fawx chat — all-in-one (recommended)
```

---

## Skill Metadata

Each skill needs metadata for the setup wizard. Add to `manifest.toml`:

```toml
[skill]
name = "weather-skill"
description = "Weather forecasts via wttr.in"

[setup]
category = "recommended"  # recommended | optional
credential_key = ""        # empty = no key needed
credential_label = ""
reuses_provider = ""       # "openai" | "anthropic" | ""
```

Examples:
- Calculator: `category="recommended"`, no credential
- TTS: `category="optional"`, `reuses_provider="openai"`
- Browser: `category="optional"`, `credential_key="brave_api_key"`, `credential_label="Brave API key"`
- GitHub: `category="optional"`, `credential_key="github_token"`, `credential_label="GitHub Personal Access Token"`

---

## Behavior

### Skill selection
- Recommended skills default to selected
- Optional skills default to unselected
- Skills that reuse a configured provider show "(uses your OpenAI key)"
- Skills that reuse a provider NOT configured show "(needs OpenAI key)" dimmed
- Arrow keys + space to toggle, enter to confirm

### Credential step
- Only shown if any selected skill requires a new credential
- Skip keys that are already stored
- Allow blank input to skip (with note: "skill will prompt for key later")
- If a skill reuses a provider key, no prompt needed — just inform the user

### Skill installation
- Build from source: `cargo build -p <skill> --target wasm32-wasip1`
- Or download pre-built from release artifacts (future)
- Install to `~/.fawx/skills/<name>/`

### Re-running setup
- `fawx setup` is idempotent
- Detects already-installed skills, already-stored credentials
- Shows current state, lets user modify

---

## Implementation Notes

### Files
- `engine/crates/fx-cli/src/commands/setup.rs` — add skill selection step
- Skill manifests — add `[setup]` section
- May need `fawx skill list --available` to enumerate built-in skills

### Interactive input
- Use the same `read_line` / terminal input pattern as existing setup steps
- For multiselect: number-based toggle (simpler than arrow keys for initial version)

### Pre-built skills
For launch, skills are built from source during setup. Pre-built WASM binaries from GitHub Releases is a fast follow.

---

## Out of scope
- Downloading skills from marketplace during setup (future)
- Skill auto-update
- Skill dependency resolution
