# Phase 3 Roadmap — Documentation, Polish & Launch

**Status:** Planning  
**Prerequisite:** Phase 2 complete (17 PRs merged to dev)  
**Goal:** Ship-ready codebase — docs, CI, landing page, integration tested, released.

---

## 3a: Documentation & OSS Prep (no dev dependency — start now)

These can all be done against current main/staging since they're docs and content.

### 1. README Rewrite
The current README references `ct-*` crate names, old architecture, and doesn't reflect the current state. Complete rewrite:
- Update project description: TUI-first agentic engine, not "phone agent"
- Fix crate names: `fx-*` not `ct-*`
- Update architecture diagram (kernel + loadable + TUI + HTTP API)
- Update features list (streaming, WASM skills, memory, edit_file, etc.)
- Update getting started (fawx setup, fawx-tui, config.toml)
- Update test counts (2,000+)
- Update roadmap to current horizons
- Remove Android-specific sections (moved to Citros)
- Add WASM skill marketplace section

### 2. Architecture Docs
- Update `ARCHITECTURE.md` to reflect current crate map
- Add HTTP API docs (endpoints, SSE streaming, auth)
- Add WASM skill development guide (host_api_v1, manifest, testing)
- Clean up `docs/` — archive obsolete specs, organize by topic

### 3. Contributing Guide
- Code standards (link to ENGINEERING.md)
- PR process (dev → staging → main)
- Skill development guide
- Issue templates

### 4. fawx.ai Landing Page
- Hero section: what Fawx is
- Features grid: skills, streaming, memory, security
- Quick start
- Links to GitHub, docs

### 5. Docs Cleanup
- Archive completed specs to `docs/archive/`
- Remove deprecated docs
- Consolidate duplicate content
- `docs/` README with index

---

## 3b: Build & Operations (targets dev branch)

### 6. Unified Build Script (#1269)
- Single `scripts/build.sh` that builds engine, TUI, and all WASM skills
- Cross-platform (Linux + macOS)
- `--release` flag for production builds
- Outputs version info

### 7. `fawx restart` Command (#1274)
- Unified stop/build/start command
- Graceful shutdown (save state, close connections)
- Build verification before restart

### 8. CI Improvements
- Add WASM skill build step to CI
- Add integration test smoke check
- Ensure CI runs against dev branch too (not just staging/main)
- Add badge for skill build status

---

## 3c: Integration Testing (needs Joe + Mac Mini)

### 9. Full Integration Test on Dev
Build dev branch on Mac Mini, run through checklist:
- Engine capabilities: edit_file, read_file offset, persistent logs, background procs, proposals
- Streaming: text streaming, tool calls, HTTP SSE
- Memory: write + semantic search (keyword fallback if no model)
- WASM skills: weather, vision, TTS, STT, browser, canvas, scheduler
- Stress: multi-tool calls, interrupt, error recovery

### 10. Dev → Staging Promotion
- Merge dev to staging after integration passes
- Verify staging CI green

---

## 3d: Release & Launch

### 11. Staging → Main Release
- Release PR with changelog
- Version bump
- Final CI check

### 12. Repo Transfer
- Transfer `abbudjoe/fawx` to `fawxai/fawx`
- Update all references (CI, docs, links)
- Set up GitHub org settings

### 13. fawx.ai Deploy
- Deploy landing page
- DNS setup
- SSL cert

### 14. Launch Fest Prep
- Demo script
- Pitch deck updates
- Recorded demo as backup

---

## Sequencing

```
NOW (no dev dependency):
  3a.1 README rewrite
  3a.2 Architecture docs
  3a.3 Contributing guide
  3a.4 Landing page content
  3a.5 Docs cleanup

PARALLEL (targets dev):
  3b.6 Build script
  3b.7 fawx restart
  3b.8 CI improvements

AFTER DOCS + JOE AVAILABLE:
  3c.9 Integration testing
  3c.10 Dev → staging

AFTER INTEGRATION:
  3d.11 Release
  3d.12 Repo transfer
  3d.13 Landing page deploy
  3d.14 Launch Fest prep
```

---

## Decisions
- **Launch date:** Wednesday March 11, 2026
- **Landing page:** Post repo transfer (not blocking launch)
- **Launch Fest:** Missed — not blocking
- **Repo transfer:** Before launch (fawxai/fawx is the public identity)
- **Public README:** Include everything built. Exclude future roadmap items. Show the safety architecture — it's the moat.

## Timeline
- **Tue 3/10:** Docs, cleanup, README rewrite, integration test on Mac Mini
- **Wed 3/11:** Repo transfer → final checks → launch
