# API Audit: CLI → HTTP Endpoint Parity

**Goal**: Every user-facing CLI/TUI function has a corresponding HTTP API
endpoint so the Swift app can fully replace the terminal.

---

## Legend

- ✅ = endpoint exists and works
- ⚠️ = partially exists (needs extension)
- ❌ = no endpoint, needs building
- 🚫 = not applicable to GUI client (server-side / daemon management)

---

## Current HTTP API (what exists today)

| Endpoint | Method | Auth | Purpose |
|----------|--------|------|---------|
| `/health` | GET | No | Status, model, uptime |
| `/message` | POST | Bearer | Send message, get response (JSON or SSE) |
| `/status` | GET | Bearer | Model, skills, memory, Tailscale IP, config |
| `/config` | GET | Bearer | Read config by section |
| `/config` | POST | Bearer | Set config key/value |
| `/webhook/{id}` | POST | Bearer | Webhook channel ingest |
| `/fleet/register` | POST | Bearer | Worker node registration |
| `/fleet/heartbeat` | POST | Bearer | Worker heartbeat |
| `/fleet/result` | POST | Bearer | Worker task result submission |

---

## CLI Commands → HTTP Mapping

### Core Chat (Phase 1 — Daily Driver)

| CLI Function | HTTP Endpoint | Status |
|-------------|---------------|--------|
| `fawx chat` / send message | `POST /message` | ✅ exists |
| SSE streaming response | `POST /message` + `Accept: text/event-stream` | ✅ exists |
| `/new` — new conversation | `POST /sessions` | ❌ **MISSING** |
| `/clear` — clear conversation | `POST /sessions/{id}/clear` | ❌ **MISSING** |
| `/history` — show history | `GET /sessions/{id}/messages` | ❌ **MISSING** |
| List conversations | `GET /sessions` | ❌ **MISSING** |
| `/model` — switch model | `PUT /model` | ❌ **MISSING** (doable via `/config` but clunky) |
| `/status` — runtime status | `GET /status` | ✅ exists |
| `/thinking` — toggle thinking | `PUT /thinking` | ❌ **MISSING** |
| Send image with message | `POST /message` (multipart or base64) | ❌ **MISSING** (text-only today) |
| Tool call display | SSE `ToolCallStart/Complete/Result` events | ✅ exists |
| Error display | SSE `Error` event | ✅ exists (PR #1391) |

### Configuration & Auth (Phase 1)

| CLI Function | HTTP Endpoint | Status |
|-------------|---------------|--------|
| `fawx config show` | `GET /config?section=all` | ✅ exists |
| `fawx config get <key>` | `GET /config?section=<key>` | ✅ exists |
| `fawx config set <key> <val>` | `POST /config` | ✅ exists |
| `fawx auth set-token` | `POST /auth/token` | ❌ **MISSING** |
| `fawx auth set-credential` | `POST /auth/credential` | ❌ **MISSING** |
| `fawx auth list-credentials` | `GET /auth/credentials` | ❌ **MISSING** |
| `fawx auth status` | `GET /auth/status` | ❌ **MISSING** |

### Skills (Phase 2)

| CLI Function | HTTP Endpoint | Status |
|-------------|---------------|--------|
| `fawx list` — installed skills | `GET /skills` | ❌ **MISSING** |
| `fawx search <q>` — registry search | `GET /skills/search?q=<q>` | ❌ **MISSING** |
| `fawx install <name>` | `POST /skills/install` | ❌ **MISSING** |
| `fawx skill remove <name>` | `DELETE /skills/{name}` | ❌ **MISSING** |
| `/skills` slash command | Same as `GET /skills` | ❌ **MISSING** |

### Experiments (Phase 2)

| CLI Function | HTTP Endpoint | Status |
|-------------|---------------|--------|
| `fawx experiment run` | `POST /experiments` | ❌ **MISSING** |
| Experiment progress (TUI panel) | `GET /experiments/{id}/events` (SSE) | ❌ **MISSING** |
| Experiment results | `GET /experiments/{id}` | ❌ **MISSING** |
| Experiment list/history | `GET /experiments` | ❌ **MISSING** |

### Fleet Management (Phase 2)

| CLI Function | HTTP Endpoint | Status |
|-------------|---------------|--------|
| `fawx fleet init` | `POST /fleet/init` | ❌ **MISSING** |
| `fawx fleet add` | `POST /fleet/nodes` | ❌ **MISSING** |
| `fawx fleet remove` | `DELETE /fleet/nodes/{name}` | ❌ **MISSING** |
| `fawx fleet list` | `GET /fleet/nodes` | ❌ **MISSING** |
| Worker registration | `POST /fleet/register` | ✅ exists |
| Worker heartbeat | `POST /fleet/heartbeat` | ✅ exists |

### Agent Intelligence (Phase 2)

| CLI Function | HTTP Endpoint | Status |
|-------------|---------------|--------|
| `/signals` — active signals | `GET /signals` | ❌ **MISSING** |
| `/budget` — budget status | `GET /budget` | ❌ **MISSING** |
| `/loop` — loop state | `GET /loop` | ❌ **MISSING** |
| `/proposals` — pending proposals | `GET /proposals` | ❌ **MISSING** |
| `/approve {id}` | `POST /proposals/{id}/approve` | ❌ **MISSING** |
| `/reject {id}` | `POST /proposals/{id}/reject` | ❌ **MISSING** |
| `/analyze` — trigger analysis | `POST /analyze` | ❌ **MISSING** |
| `/improve` — self-improvement | `POST /improve` | ❌ **MISSING** |
| `/debug` — debug info | `GET /debug` | ❌ **MISSING** |

### Administration (Phase 2/3)

| CLI Function | HTTP Endpoint | Status |
|-------------|---------------|--------|
| `fawx doctor` | `GET /diagnostics` | ❌ **MISSING** |
| `fawx logs` | `GET /logs` | ❌ **MISSING** |
| `fawx audit` | `GET /audit` | ❌ **MISSING** |
| `fawx backup` | `POST /backup` | ❌ **MISSING** |
| `fawx update` | `POST /update` | ❌ **MISSING** |
| `fawx restart` | `POST /restart` | ❌ **MISSING** |
| `fawx reset` | `POST /reset` | ❌ **MISSING** |
| `fawx version` | `GET /version` | ⚠️ partial (in `/health`) |
| Journal search (direct) | `GET /journal?q=<q>` | ❌ **MISSING** |

### Not Applicable to GUI

| CLI Function | Why |
|-------------|-----|
| `fawx start` / `fawx stop` | Daemon management — server must already be running |
| `fawx tui` | The Swift app IS the new TUI |
| `fawx serve` | Server-side — the app connects to a running serve |
| `fawx completions` | Shell-specific |
| `fawx oauth-bridge` | Android-specific auth flow |
| `fawx setup` wizard | Can be rebuilt as in-app onboarding using auth + config endpoints |
| `fawx skill build/create` | Developer tooling, not end-user |
| `fawx eval-determinism` | CI tooling |

---

## Summary

| Category | Total | ✅ Exists | ❌ Missing |
|----------|-------|----------|-----------|
| Phase 1 (Daily Driver) | 14 | 6 | 8 |
| Phase 2 (Power User) | 24 | 2 | 22 |
| Total | 38 | 8 | 30 |

### Phase 1 Missing Endpoints (8) — required for daily driver

1. `POST /sessions` — create new conversation
2. `GET /sessions` — list conversations
3. `GET /sessions/{id}/messages` — conversation history
4. `POST /sessions/{id}/clear` — clear conversation
5. `PUT /model` — switch active model
6. `PUT /thinking` — toggle thinking mode
7. `POST /message` extension — accept `session_id` + `images` (base64 array)
8. Auth management endpoints (4 sub-endpoints: set-token, set-credential, list-credentials, status)

### Phase 2 Missing Endpoints (22) — required for full parity

Skills (4), Experiments (4), Fleet management (4), Agent intelligence (7), Administration (7)

---

## Recommended Build Order

### Sprint 1: Session + Chat enrichment (~200 lines)
Sessions CRUD + message history + image support on `/message`

### Sprint 2: Model + Thinking + Auth (~150 lines)
Model switching, thinking toggle, auth credential management

### Sprint 3: Skills + Fleet admin (~200 lines)
Skill CRUD + search, fleet init/add/remove/list

### Sprint 4: Experiments + Agent intelligence (~300 lines)
Experiment launch/status/history, signals/budget/proposals

### Sprint 5: Administration (~150 lines)
Diagnostics, logs, audit, backup, update, restart
