# Phase 3 Roadmap — Documentation, Polish & Launch

**Status:** Final Sprint  
**Prerequisite:** Phase 2 complete (17 PRs), Phase 3a-3b nearly complete  
**Goal:** Ship-ready codebase — docs, ops, polish, integration tested, released.  
**Launch target:** Wednesday March 11, 2026

---

## Progress Summary — Session 4 (March 9-10, 2026)

### Phase 2 — ALL COMPLETE ✅ (17 PRs merged)
- 2a: edit_file, persistent logs, background processes, streaming P1, proposals UX
- 2a+: Streaming P2 (engine + HTTP SSE)
- 2b: 7 WASM skills (weather, vision, TTS, STT, browser, canvas, scheduler) + skill migration
- 2c: Engine intelligence (embeddings, embedding index, memory_search)

### Phase 3a — Docs ✅ COMPLETE
- [x] README.md rewrite (315 lines)
- [x] ARCHITECTURE.md rewrite (34-crate map)
- [x] CONTRIBUTING.md (new)
- [x] 63 docs archived
- [x] Docs index added

### Phase 3b — Ops ✅ COMPLETE (7 PRs merged)
- [x] PR #1304: Build script + `fawx restart`
- [x] PR #1305: CLI ops (doctor, status, version, logs, security-audit) + Telegram commands + plain text fix
- [x] PR #1312: `skills/build.sh --install`
- [x] PR #1313: Proposal gate security fix
- [x] PR #1315: `fawx update` command + restart --rebuild enhancement
- [x] PR #1316: `fawx chat` → embedded TUI (no server needed)
- [x] PR #1317: `fawx import` + `fawx backup` (lossless migration, all .md files)
- [x] PR #1318: Planner resilience (skip failed candidates instead of aborting)

### Hotfixes (direct commits to dev, reviewed)
- TUI SSE wire format alignment (#1314)
- SSE dispatch tests + dead serde attr cleanup
- Slash commands display response
- Duplicate response text prevention
- dispatch_sse_frame decomposition (40-line compliance)
- DEFAULT_OPENAI_MODELS update
- TUI Accept header fix for streaming
- Bash empty array expansion on macOS

### Integration Testing (Mac Mini) — PARTIAL
- [x] Build on Mac Mini
- [x] `fawx setup` wizard
- [x] `fawx serve --http` + Telegram channel
- [x] Skills built (8/8)
- [x] Model switch mid-conversation
- [x] SSE streaming in TUI
- [x] Slash commands in Telegram
- [x] `fawx update dev` (dirty tree check)
- [ ] Slash commands in TUI (fix pushed, needs pull+rebuild)
- [ ] Skill install + WASM skill test
- [ ] Proposal gate test
- [ ] File operations test
- [ ] Background process test
- [ ] Memory test

---

## What's Left Before Launch

### Must-Have (Blocking Launch)

#### 1. Integration Testing — Finish Checklist
Pull latest dev on Mac Mini, rebuild, complete remaining tests:
- Slash commands in TUI
- WASM skill execution (weather, etc.)
- Proposal gate (write to protected file, verify proposal)
- File operations (edit_file, read_file offset)
- Background processes
- Memory (journal_write + journal_search)

#### 2. Dev → Staging Promotion
After integration passes, merge dev → staging.

#### 3. OSS Repo Scrub
Before making public, remove:
- `docs/archive/backlog-specs/` (internal planning)
- `docs/specs/phase3-roadmap.md` (internal roadmap)
- `TASTE.md` (internal preferences)
- Any "OS transition" / "Horizon" / VC / Launch Fest references
- Cleanest approach: fresh `fawxai/fawx` repo with squashed initial commit from dev

#### 4. Staging → Main Release
- Release PR with changelog
- Version bump
- Final CI check

#### 5. Repo Transfer to fawxai Org
- Create `fawxai/fawx` (public)
- `fawxai/fawxtui` already exists (public)
- Update references in docs, CI, links

### Nice-to-Have (Can Ship After Launch)

#### 6. Phase 3c — Polish
- TUI welcome screen redesign (spec: `docs/specs/tui-welcome-screen.md`)
- CLI output polish V2 (spec: `docs/specs/cli-output-polish.md`)

#### 7. fawx.ai Landing Page
- HTML received from Cowork, needs review/revision
- Fox mascot ready
- Deploy after repo is public (links need to resolve)

#### 8. Engine Context Loading
- Load all .md files from `~/.fawx/context/` into system prompt
- Makes import fully functional (files are copied but not yet read by engine)

#### 9. Fast Follow-ups
- #1298: Shell completions (bash/zsh/fish)
- #1299: `fawx config set KEY VALUE`
- #1300: `fawx skill create` scaffolding

---

## CLI Feature Complete ✅

```
fawx setup          — first-run wizard
fawx chat           — embedded TUI (no server needed) ← NEW
fawx serve          — headless/HTTP mode
fawx tui            — TUI (connects to HTTP server)
fawx start/stop/restart — daemon control
fawx update [BRANCH] — pull + build all + restart
fawx import --from openclaw — migrate from OpenClaw (lossless)
fawx backup         — backup ~/.fawx/ to tar.gz
fawx doctor         — diagnostics
fawx status         — runtime status
fawx version        — version + git hash + build date
fawx logs           — tail persistent logs
fawx security-audit — full security audit
fawx config         — show config
fawx auth           — credential management
fawx skill install/list/sign/verify — skill management
fawx audit show/verify — audit log
fawx search/install — marketplace
```

---

## Timeline
- **Tue 3/10 (now):** Finish integration testing, dev → staging
- **Wed 3/11:** OSS scrub, repo transfer, release, launch
