# Fleet Node Setup — Spec

**Status:** Draft  
**Author:** Clawdio + Joe  
**Date:** 2026-03-12  

---

## Problem

Fawx experiments currently run on a single machine. The consensus protocol supports multi-node evaluation but has no way to register, authenticate, or dispatch work to remote nodes. We need a setup flow that lets machines join a fleet and receive experiment tasks.

## Design Principles

1. **Tailnet-only** — all fleet communication over Tailscale. No public internet exposure.
2. **Token-level security** — nodes authenticate with issued tokens, not SSH keys.
3. **Primary orchestrates, workers compute** — only the primary touches GitHub's write API.
4. **Zero-install workers** — worker setup is one command after Rust toolchain exists.
5. **PAT borrowing** — workers receive a scoped GitHub PAT per task for repo cloning.

---

## Architecture

```
┌─────────────────────────────────────┐
│           Primary Node              │
│  (VPS / Mac Mini / any machine)     │
│                                     │
│  ┌─────────────┐  ┌──────────────┐  │
│  │ Orchestrator │  │ Fleet Manager│  │
│  │ (consensus)  │  │ (registry,   │  │
│  │              │  │  dispatch,   │  │
│  │              │  │  health)     │  │
│  └──────┬───────┘  └──────┬───────┘  │
│         │                 │          │
│  ┌──────┴─────────────────┴───────┐  │
│  │     GitHub (full PAT)          │  │
│  │     Chain store                │  │
│  │     fawx serve (HTTP API)      │  │
│  └────────────────────────────────┘  │
└─────────────┬───────────────────────┘
              │ Tailscale
     ┌────────┼────────┐
     │        │        │
┌────┴───┐ ┌──┴────┐ ┌─┴───────┐
│Worker 1│ │Worker 2│ │Worker N │
│Mac Mini│ │MacBook │ │Future   │
│        │ │        │ │GPU box  │
│fawx    │ │fawx    │ │fawx     │
│serve   │ │serve   │ │serve    │
│--fleet │ │--fleet │ │--fleet  │
└────────┘ └───────┘  └─────────┘
```

---

## Components

### 1. Fleet Token

A signed JWT or HMAC token issued by the primary. Contains:

```json
{
  "node_id": "macmini-01",
  "issued_at": "2026-03-12T03:55:00Z",
  "issued_by": "primary",
  "capabilities": ["generate", "evaluate"],
  "scopes": ["experiment"]
}
```

**Storage:**
- Primary: `~/.fawx/fleet/tokens.json` (issued tokens, revocation list)
- Worker: `~/.fawx/fleet/identity.json` (own token + primary endpoint)

**Security properties:**
- Tokens are bearer tokens over Tailscale (encrypted transport)
- Primary can revoke any token instantly
- Tokens don't grant GitHub access — PAT is per-task, per-clone

### 2. Fleet Signing Key

Generated on `fawx fleet init`. Used to sign/verify fleet tokens.

```
~/.fawx/fleet/
├── fleet_key.pem        # signing key (primary only)
├── fleet_key.pub        # verification key (distributed to workers)
├── tokens.json          # issued token registry
└── nodes.json           # registered node metadata
```

Workers receive the public key during `fawx fleet join` and use it to verify task payloads from the primary.

### 3. Node Registry

The primary maintains a registry of known nodes:

```json
{
  "nodes": [
    {
      "node_id": "macmini-01",
      "name": "Mac Mini",
      "tailscale_ip": "100.75.191.19",
      "port": 8400,
      "capabilities": {
        "cpus": 8,
        "ram_gb": 16,
        "gpu": null,
        "rust_version": "1.85.0",
        "os": "macos-arm64"
      },
      "status": "online",
      "last_heartbeat": "2026-03-12T03:55:00Z",
      "token_id": "tok_abc123"
    }
  ]
}
```

### 4. GitHub PAT Distribution

**Primary holds:**
- Full PAT: `repo` scope (read + write) for PRs, commits, pushes

**Workers receive per-task:**
- Read-only PAT: `contents:read` fine-grained PAT, or the full PAT if fine-grained isn't set up
- Delivered in the task payload, used only for `git clone`
- Worker sets it as `GIT_ASKPASS` or credential helper for the clone, never persists it

**Config:**
```toml
# ~/.fawx/config.toml (primary)
[fleet]
enabled = true
role = "primary"
github_pat_worker = "ghp_readonly..."   # scoped read-only PAT for workers
github_pat_primary = "ghp_full..."      # full PAT (existing auth)

# ~/.fawx/config.toml (worker)
[fleet]
enabled = true
role = "worker"
primary_endpoint = "http://100.93.251.101:8400"
token_path = "~/.fawx/fleet/identity.json"
```

---

## Setup Flow

### Primary Initialization

```bash
$ fawx fleet init
✓ Generated fleet signing key at ~/.fawx/fleet/fleet_key.pem
✓ Fleet manager started on port 8400
✓ Ready to add nodes.

$ fawx fleet add macmini --ip 100.75.191.19
✓ Token generated for node "macmini"
✓ Join command:

  fawx fleet join 100.93.251.101:8400 --token eyJ...

  Run this on the worker machine.
```

### Worker Join

```bash
$ fawx fleet join 100.93.251.101:8400 --token eyJ...
✓ Connected to primary at 100.93.251.101:8400
✓ Registered as node "macmini"
✓ Received fleet public key
✓ Reporting capabilities: 8 CPUs, 16GB RAM, rust 1.85.0, macos-arm64
✓ Ready for tasks. Starting fleet worker...

  fawx serve --fleet is now running.
  Primary will dispatch experiment tasks to this node.
```

### What `fleet join` does:
1. Validates the token against the primary's `/fleet/verify` endpoint
2. Receives the fleet public key for future task verification
3. Reports node capabilities (auto-detected)
4. Stores identity to `~/.fawx/fleet/identity.json`
5. Starts `fawx serve --fleet` (listens for task dispatch)

---

## Task Dispatch Protocol

### Experiment Flow

```
Primary                              Worker
   │                                    │
   │  POST /fleet/task                  │
   │  {                                 │
   │    task_id: "exp-001",             │
   │    type: "generate" | "evaluate",  │
   │    repo_url: "https://...",        │
   │    branch: "dev",                  │
   │    git_token: "ghp_readonly...",   │
   │    signal: { ... },               │
   │    config: { ... },               │
   │    chain_history: [ ... ],         │
   │    signed: "<signature>"           │
   │  }                                 │
   ├───────────────────────────────────►│
   │                                    │  1. Verify signature
   │                                    │  2. Clone repo (temp dir)
   │                                    │  3. Run generator/evaluator
   │                                    │  4. Collect results
   │  POST /fleet/result (callback)     │
   │  {                                 │
   │    task_id: "exp-001",             │
   │    status: "complete",             │
   │    candidate_patch: "...",         │
   │    evaluation: { ... },            │
   │    build_log: "...",               │
   │    signed: "<signature>"           │
   │  }                                 │
   │◄───────────────────────────────────┤
   │                                    │  5. Cleanup temp dir
   │                                    │  6. Discard git_token
```

### Task Types

| Type | Input | Output |
|------|-------|--------|
| `generate` | Signal, scope, chain history | Candidate patch |
| `evaluate` | Candidate patch, signal | Evaluation (score, build log, test results) |
| `generate_and_evaluate` | Signal, scope, chain history | Candidate + self-eval (for single-node) |

### Endpoints (Worker)

```
POST /fleet/task          # Receive a task from primary
GET  /fleet/status        # Health check + current task status
POST /fleet/cancel        # Cancel current task
GET  /fleet/capabilities  # Report node capabilities
```

### Endpoints (Primary)

```
POST /fleet/register      # Worker registration
POST /fleet/heartbeat     # Worker heartbeat
POST /fleet/result        # Receive task result from worker
GET  /fleet/nodes         # List registered nodes
POST /fleet/verify        # Verify a fleet token
```

---

## Health & Availability

### Heartbeat

Workers send heartbeats every 30 seconds:

```json
POST /fleet/heartbeat
{
  "node_id": "macmini-01",
  "status": "idle" | "busy",
  "current_task": "exp-001" | null,
  "load_avg": 2.1,
  "available_disk_gb": 45
}
```

Primary marks nodes as `offline` after 3 missed heartbeats (90 seconds).

### Node States

```
registered → online → busy → online → offline
                ↑                        │
                └────────────────────────┘
                      (reconnect)
```

---

## Security Model

### Trust Boundaries

1. **Tailscale** — network-level encryption and identity. Only Tailnet members can reach fleet endpoints.
2. **Fleet tokens** — application-level auth. Signed by primary, verified by both sides.
3. **Task signatures** — each task payload is signed by the primary's fleet key. Workers verify before executing.
4. **PAT scoping** — workers get read-only repo access. Can clone and test, cannot push or PR.
5. **Ephemeral credentials** — git token is used for one clone, then discarded. Never written to disk.

### What workers CANNOT do:
- Push to any branch
- Open or modify PRs
- Access other repos (PAT is scoped)
- Impersonate the primary
- Accept tasks from non-primary sources (signature verification)

### What the primary CANNOT do to workers:
- Execute arbitrary commands (workers only accept typed task payloads)
- Access worker filesystem outside the temp experiment dir
- Persist credentials on the worker

---

## CLI Commands

```bash
# Primary
fawx fleet init                        # Generate fleet key, enable fleet mode
fawx fleet add <name> [--ip <ip>]      # Issue token, print join command
fawx fleet remove <name>               # Revoke token, deregister node
fawx fleet list                        # Show registered nodes + status
fawx fleet revoke <name>               # Revoke token without removing
fawx fleet status                      # Fleet health overview

# Worker
fawx fleet join <primary> --token <t>  # Register with primary
fawx fleet leave                       # Deregister from primary
fawx fleet status                      # Show connection to primary

# Experiment (existing, updated)
fawx experiment run --fleet            # Use all available fleet nodes
fawx experiment run --nodes macmini    # Use specific nodes
fawx experiment run --nodes 3          # Use best N available nodes
```

---

## Config Schema

```toml
# Primary
[fleet]
enabled = true
role = "primary"
port = 8400                            # fleet API port (same as fawx serve)
github_pat_worker = "ghp_..."          # read-only PAT distributed to workers
heartbeat_interval_secs = 30
offline_threshold_missed = 3

# Worker
[fleet]
enabled = true
role = "worker"
primary_endpoint = "http://100.93.251.101:8400"
token_path = "~/.fawx/fleet/identity.json"
max_concurrent_tasks = 1               # how many experiments to run in parallel
workspace_dir = "/tmp/fawx-fleet"      # temp dir for experiment clones
```

---

## Implementation Phases

### Phase 1: Fleet Init + Join (foundation)
- `fawx fleet init` — key generation, config
- `fawx fleet add` — token issuance
- `fawx fleet join` — registration, capability reporting
- `fawx fleet list` / `fawx fleet status`
- New crate: extend `fx-fleet` with `FleetManager`, `FleetToken`, `NodeRegistry`

### Phase 2: Task Dispatch (core)
- Worker HTTP endpoints (`/fleet/task`, `/fleet/status`)
- Primary HTTP endpoints (`/fleet/result`, `/fleet/heartbeat`)
- `HttpTransport` for `NodeTransport` trait (replaces SSH for fleet nodes)
- PAT distribution in task payload
- Task signature + verification

### Phase 3: Experiment Integration (wiring)
- `--fleet` flag on `fawx experiment run`
- Scheduler: assign generate/evaluate tasks to available nodes
- Results flow back through existing consensus protocol
- Progress events forwarded from workers to primary (→ TUI panel / Telegram)

### Phase 4: Reliability (hardening)
- Task retry on worker failure
- Heartbeat monitoring + automatic failover
- Task timeout + cleanup
- Graceful shutdown (finish current task, then leave)

---

## Open Questions

1. **Single PAT vs per-worker PATs?** Single is simpler but can't revoke per-node. Fine-grained PATs could be scoped per-worker.
2. **Worker auto-update?** Should the primary push Fawx binary updates to workers, or is that manual?
3. **Result caching?** If two workers evaluate the same patch, should results be cached/deduped?
4. **Non-Rust experiments?** Current system assumes `cargo build` + `cargo test`. Future experiments might need Python, training pipelines, etc. Should task types be extensible?
5. **Fleet across Tailnets?** Is it always one Tailnet, or could nodes from different Tailnets join via shared nodes?
