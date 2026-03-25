# fx-cloud-gpu: Cloud GPU Provider Trait + RunPod Skill

**Date:** 2026-03-25
**Status:** Spec — ready for implementation

---

## Two pieces:

1. **CloudGpuProvider trait** — engine crate (`fx-cloud-gpu`), defines the interface
2. **RunPod skill** — WASM skill implementing the trait via RunPod API

---

## Part 1: Engine Trait (`fx-cloud-gpu`)

### New crate: `engine/crates/fx-cloud-gpu/`

```
engine/crates/fx-cloud-gpu/
  Cargo.toml
  src/
    lib.rs      # CloudGpuProvider trait + types
```

### Types

```rust
pub struct PodConfig {
    pub name: String,
    pub gpu: GpuType,
    pub gpu_count: u32,
    pub image: String,
    pub disk_gb: u32,
    pub env: HashMap<String, String>,
}

pub enum GpuType {
    Rtx3090,
    Rtx4090,
    A100_80gb,
    H100_80gb,
    Custom(String),
}

pub struct Pod {
    pub id: String,
    pub status: PodStatus,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub gpu: GpuType,
    pub cost_per_hour: f64,
}

pub enum PodStatus {
    Creating,
    Running,
    Stopped,
    Terminated,
}

pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}
```

### Trait

```rust
#[async_trait]
pub trait CloudGpuProvider: Send + Sync {
    fn provider_name(&self) -> &str;

    async fn create_pod(&self, config: PodConfig) -> Result<Pod, GpuError>;
    async fn list_pods(&self) -> Result<Vec<Pod>, GpuError>;
    async fn pod_status(&self, pod_id: &str) -> Result<Pod, GpuError>;
    async fn stop_pod(&self, pod_id: &str) -> Result<(), GpuError>;
    async fn destroy_pod(&self, pod_id: &str) -> Result<(), GpuError>;

    async fn exec(&self, pod_id: &str, command: &str, timeout_seconds: u32) -> Result<ExecResult, GpuError>;
    async fn upload(&self, pod_id: &str, local_path: &Path, remote_path: &str) -> Result<(), GpuError>;
    async fn download(&self, pod_id: &str, remote_path: &str, local_path: &Path) -> Result<(), GpuError>;
}
```

### CloudGpuSkill (built-in)

Wraps a `Box<dyn CloudGpuProvider>` and exposes tools:
- `gpu_create`, `gpu_list`, `gpu_status`, `gpu_stop`, `gpu_destroy`
- `gpu_exec`, `gpu_upload`, `gpu_download`

Registered at startup if a provider is configured.

---

## Part 2: Host API Extension (prerequisite for WASM RunPod skill)

Add to `fx-skills` Capability enum:
```rust
Shell,         // execute system commands
Filesystem,    // read/write local files
```

Add to `HostApi` trait:
```rust
fn exec_command(&self, command: &str, args: &str, timeout_ms: u32) -> Option<String>;
fn read_file(&self, path: &str) -> Option<String>;
fn write_file(&self, path: &str, content: &str) -> bool;
```

Implement in `LiveHostApi`, gated behind capability check.

Add to WASM linker in `fx-skills` runtime.

---

## Part 3: RunPod WASM Skill

### Skill repo: `fawxai/runpod`

```
fawxai/runpod/
  manifest.toml
  src/
    lib.rs      # WASM entry, tool routing
    api.rs      # RunPod REST API (uses http_request)
    ssh.rs      # SSH exec/transfer (uses exec_command for ssh/scp)
    cost.rs     # Spend tracking
```

### manifest.toml
```toml
name = "runpod"
version = "0.1.0"
description = "Provision GPU pods on RunPod for ML experiments"
author = "fawxai"
api_version = "host_api_v1"
capabilities = ["network", "shell", "filesystem"]
entry_point = "run"

[credentials]
runpod_api_key = { description = "RunPod API key", required = true }
```

### API calls (via http_request — works today):
- `POST https://api.runpod.io/v2/pods` — create
- `GET https://api.runpod.io/v2/pods` — list
- etc.

### SSH (needs host_exec_command):
- `exec_command("ssh", "-p {port} root@{host} '{command}'", timeout)`
- `exec_command("scp", "-P {port} {local} root@{host}:{remote}", timeout)`

---

## Subtask Decomposition

### Task 1: fx-cloud-gpu trait crate (~100 lines)
- Types: PodConfig, Pod, PodStatus, GpuType, ExecResult, GpuError
- CloudGpuProvider trait
- CloudGpuSkill wrapper (Skill impl, tool definitions, routing)
- Tests: mock provider, tool routing

### Task 2: Host API extension (~150 lines)
- Add Shell, Filesystem to Capability enum in fx-skills
- Add exec_command, read_file, write_file to HostApi trait
- Implement in LiveHostApi with capability gating
- Add to WASM linker
- Tests: capability gating, exec, file read/write

### Task 3: RunPod WASM skill — API layer (~120 lines)
- RunPod REST client (create, list, status, stop, destroy)
- Cost tracking per session
- Tests with mock HTTP responses

### Task 4: RunPod WASM skill — SSH layer (~100 lines)
- SSH exec wrapper
- SCP upload/download
- SSH key management
- Tests with mock exec_command

### Task 5: Integration + signing
- End-to-end flow
- Sign + publish to registry
