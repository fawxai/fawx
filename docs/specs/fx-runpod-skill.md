# fx-runpod: GPU Cloud Execution Skill

**Date:** 2026-03-25
**Status:** Spec

---

## Problem

ML experiments need GPUs. Fawx has no way to provision cloud compute. For parameter-golf, TurboQuant experiments, or any training/inference workload, Fawx needs to spin up GPU instances, run code, collect results, and tear down.

RunPod is the target: cheap GPUs, simple API, pay-per-second billing.

## Architecture

WASM skill that manages RunPod pods through their REST API. Provides tools for pod lifecycle, file transfer, and remote execution.

### Host API Capabilities Required
- `network` — RunPod API calls + SSH
- `shell` — scp/rsync for file transfer, ssh for remote exec
- `filesystem` — read local files to upload, write downloaded results
- `credentials` — RunPod API key storage

### Tools Exposed

#### `runpod_create`
Create a GPU pod.

```json
{
  "name": "turboquant-exp-1",
  "gpu": "RTX_4090",
  "gpu_count": 1,
  "image": "runpod/pytorch:2.6.0-py3.12-cuda12.8-devel-ubuntu22.04",
  "disk_gb": 50,
  "volume_gb": 0,
  "env": {"HF_TOKEN": "..."}
}
```

**Returns:**
```json
{
  "pod_id": "abc123",
  "status": "RUNNING",
  "ssh_host": "ssh.runpod.io",
  "ssh_port": 22345,
  "gpu": "RTX_4090",
  "cost_per_hour": 0.44
}
```

**GPU options (common):**
- `RTX_4090` — $0.44/hr, 24GB VRAM, good for most experiments
- `A100_80GB` — $1.64/hr, 80GB VRAM, large models
- `H100_80GB` — $3.29/hr, 80GB VRAM, fastest training
- `RTX_3090` — $0.22/hr, 24GB VRAM, budget option

#### `runpod_exec`
Run a command on a pod via SSH.

```json
{
  "pod_id": "abc123",
  "command": "cd /workspace && python train.py --epochs 10",
  "timeout_seconds": 3600,
  "capture_output": true
}
```

**Returns:**
```json
{
  "stdout": "Epoch 1/10: loss=0.342...",
  "stderr": "",
  "exit_code": 0,
  "duration_ms": 187000
}
```

#### `runpod_upload`
Upload files to a pod.

```json
{
  "pod_id": "abc123",
  "local_path": "~/.fawx/experiments/turboquant/train.py",
  "remote_path": "/workspace/train.py"
}
```

Also supports uploading a directory (recursive).

#### `runpod_download`
Download files/artifacts from a pod.

```json
{
  "pod_id": "abc123",
  "remote_path": "/workspace/results/",
  "local_path": "~/.fawx/experiments/turboquant/results/"
}
```

#### `runpod_status`
Check pod status and cost.

```json
{
  "pod_id": "abc123"
}
```

**Returns:**
```json
{
  "pod_id": "abc123",
  "status": "RUNNING",
  "uptime_seconds": 3600,
  "cost_incurred": 0.44,
  "gpu": "RTX_4090",
  "gpu_utilization": 87
}
```

#### `runpod_stop`
Stop (pause) a pod. Storage persists, no GPU billing.

#### `runpod_destroy`
Terminate and delete a pod. All data lost.

#### `runpod_list`
List all active pods.

### Credential Management

- RunPod API key stored via Fawx credential store (encrypted)
- Setup: agent prompts user for API key on first use, stores via `store_credential`
- SSH key: generate per-pod or use a stored key

### Cost Safety

- `runpod_create` returns cost_per_hour prominently
- `runpod_status` shows cost_incurred
- Agent should confirm with user before creating expensive pods (H100s)
- Auto-destroy after idle timeout (configurable, default 30 min no SSH activity)
- Skill tracks total spend per session and warns at thresholds

## Implementation

### WASM Skill Structure
```
fawxai/runpod/
  manifest.toml
  src/lib.rs          # WASM entry point, tool registration
  src/api.rs          # RunPod REST API client
  src/pod.rs          # Pod lifecycle (create/stop/destroy)
  src/transfer.rs     # File upload/download via scp
  src/exec.rs         # Remote command execution via ssh
  src/cost.rs         # Cost tracking + safety
```

### manifest.toml
```toml
[skill]
name = "runpod"
version = "0.1.0"
description = "Provision and manage GPU pods on RunPod for ML experiments"
author = "fawxai"

[capabilities]
network = true
shell = true
filesystem = true
credentials = true

[credentials]
runpod_api_key = { description = "RunPod API key", required = true }
```

### RunPod API
- Base URL: `https://api.runpod.io/v2`
- Auth: Bearer token (API key)
- Endpoints: 
  - `POST /pods` — create
  - `GET /pods` — list
  - `GET /pods/{id}` — status
  - `POST /pods/{id}/stop` — stop
  - `DELETE /pods/{id}` — destroy
- GraphQL API also available (more features); REST preferred for simplicity

### Key Design Decisions
- **WASM skill:** Same reasoning as fx-python. Pluggable, upgradable, doesn't require engine changes.
- **SSH for exec/transfer:** RunPod provides SSH access to every pod. More reliable than their exec API for long-running commands.
- **Cost safety is mandatory:** GPU time adds up fast. The skill must make cost visible and provide guardrails.
- **No persistent volumes in v1:** Keep it simple. Upload code, run, download results, destroy. Persistent volumes can come later.
- **Auto-destroy idle pods:** Forgetting to tear down a pod is the #1 way to waste money. Default idle timeout prevents this.

## Subtask Decomposition

### Task 1: Skill scaffold + API client (~120 lines)
- manifest.toml
- WASM entry point with tool registration
- RunPod API client (auth, create, list, status, stop, destroy)
- Tests: mock API responses for each endpoint

### Task 2: Pod lifecycle tools (~100 lines)
- `runpod_create` with GPU selection and image config
- `runpod_stop`, `runpod_destroy`, `runpod_list`, `runpod_status`
- Cost tracking per session
- Tests: create/stop/destroy lifecycle

### Task 3: File transfer (~80 lines)
- `runpod_upload` and `runpod_download` via scp
- Directory support (recursive)
- SSH key management (generate or use stored)
- Tests: upload file, download file, upload directory

### Task 4: Remote execution (~80 lines)
- `runpod_exec` via SSH with timeout
- stdout/stderr capture
- Long-running command support (background + poll)
- Tests: run command, timeout, capture output

### Task 5: Cost safety + integration
- Idle pod auto-destroy (background check)
- Session spend tracking + threshold warnings
- End-to-end test: create pod → upload code → run → download results → destroy
- Sign WASM binary, publish to registry
