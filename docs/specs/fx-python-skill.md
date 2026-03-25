# fx-python: Built-in Python Execution Skill

**Date:** 2026-03-25
**Status:** Spec — ready for implementation

---

## Architecture

Built-in engine skill (like `SessionMemorySkill`, `NotifySkill`). Registered at startup in `build_skill_registry()`. Native Rust, full system access. Ships with every Fawx build.

Three tools: `python_run`, `python_install`, `python_venvs`.

## Implementation

### New crate: `engine/crates/fx-python/`

```
engine/crates/fx-python/
  Cargo.toml
  src/
    lib.rs          # PythonSkill struct + Skill trait impl
    venv.rs         # VenvManager — create/list/delete/info
    runner.rs       # run Python code, capture output + artifacts
    installer.rs    # pip install wrapper
```

### Cargo.toml
```toml
[package]
name = "fx-python"
version = "0.1.0"
edition = "2021"

[dependencies]
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["process", "time", "fs"] }
fx-kernel = { path = "../fx-kernel" }
fx-loadable = { path = "../fx-loadable" }
```

### PythonSkill (lib.rs)

```rust
pub struct PythonSkill {
    venv_root: PathBuf,       // ~/.fawx/venvs/
    experiments_root: PathBuf, // ~/.fawx/experiments/
}
```

Implements `Skill` trait:
- `name()` → `"python"`
- `tool_definitions()` → 3 tools
- `execute()` routes to `python_run`, `python_install`, `python_venvs`

### Tool: python_run

**Input:**
```json
{
  "code": "import torch; print(torch.cuda.is_available())",
  "venv": "parameter-golf",
  "timeout_seconds": 300
}
```

**Behavior:**
1. If venv doesn't exist, auto-create with `python3 -m venv`
2. Write `code` to `{experiments_root}/{venv}/run_{timestamp}.py`
3. Snapshot file mtimes in working dir
4. Execute: `{venv_root}/{venv}/bin/python {script_path}`
5. Capture stdout/stderr (cap at 512KB each)
6. Diff mtimes to find new/modified files → artifacts list
7. Kill on timeout

**Output:**
```json
{
  "stdout": "True\n",
  "stderr": "",
  "exit_code": 0,
  "artifacts": ["output/loss_curve.png"],
  "duration_ms": 1240
}
```

### Tool: python_install

**Input:**
```json
{
  "packages": ["torch", "numpy"],
  "venv": "parameter-golf",
  "requirements_file": null
}
```

**Behavior:**
1. Create venv if missing
2. Run `{venv}/bin/pip install {packages}` or `pip install -r {requirements_file}`
3. Parse installed versions from pip output

**Output:**
```json
{
  "installed": ["torch==2.6.0", "numpy==2.2.1"],
  "duration_ms": 34500
}
```

### Tool: python_venvs

**Input:**
```json
{
  "action": "list"
}
```

**Actions:** `list` (names + sizes), `delete` (remove venv dir), `info` (packages in venv)

### Registration

In `startup.rs`, after `SessionMemorySkill`:
```rust
let python_skill = fx_python::PythonSkill::new(data_dir);
registry.register(Arc::new(python_skill));
```

### Tests

1. `create_venv_on_first_run` — auto-creates venv dir
2. `run_simple_code` — `print(1+1)` → stdout "2\n"
3. `run_captures_stderr` — code that writes to stderr
4. `run_timeout_kills_process` — infinite loop killed
5. `install_package` — pip install succeeds (mock or real)
6. `list_venvs` — returns created venvs
7. `delete_venv` — removes directory
8. `artifact_detection` — code that writes a file, appears in artifacts
