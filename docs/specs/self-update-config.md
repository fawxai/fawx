# Spec: Self-Update & Config Management

**Status:** Draft  
**Date:** 2026-03-08  

---

## 1. Problem

Fawx can't manage its own configuration or restart itself. An autonomous agent needs to update config values, reload skills, and restart cleanly — especially when running as a headless server.

## 2. Goals

1. **Config read** — inspect current configuration
2. **Config patch** — modify individual config values safely
3. **Config validation** — reject invalid config before applying
4. **Graceful restart** — restart the server process after config changes
5. **Status introspection** — uptime, model, skills loaded, memory usage

## 3. Architecture

### Tools

```json
{
  "name": "config_get",
  "description": "Read current Fawx configuration",
  "parameters": {
    "section": "string — config section (model, telegram, http, etc.) or 'all'"
  }
}

{
  "name": "config_set",
  "description": "Update a configuration value. Validates before applying.",
  "parameters": {
    "key": "string — dot-separated path (e.g. 'model.default_model')",
    "value": "string — new value"
  }
}

{
  "name": "fawx_restart",
  "description": "Gracefully restart the Fawx server. Use after config changes.",
  "parameters": {
    "reason": "string — why restarting",
    "delay_seconds": "integer — delay before restart (default: 2)"
  }
}

{
  "name": "fawx_status",
  "description": "Get server status: uptime, model, skills, memory, sessions",
  "parameters": {}
}
```

### Config Management

```rust
pub struct ConfigManager {
    config_path: PathBuf,
    current: FawxConfig,
}

impl ConfigManager {
    /// Read a config section.
    pub fn get(&self, section: &str) -> Result<serde_json::Value>;

    /// Update a config value. Validates the new config before writing.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()>;

    /// Write current config to disk.
    pub fn save(&self) -> Result<()>;

    /// Reload config from disk.
    pub fn reload(&mut self) -> Result<()>;
}
```

### Restart Mechanism

For headless/HTTP mode:
1. Write updated config to disk
2. Send `SIGHUP` to self → triggers graceful shutdown
3. Wrapper script (systemd, supervisor, or shell loop) restarts the process
4. New process loads updated config

Alternative: `exec()` syscall to replace the process in-place (like OpenClaw's restart).

### Config Validation

Before writing:
- Parse the TOML to verify syntax
- Deserialize into `FawxConfig` to verify types
- Check model name exists in available models (if changing default_model)
- Check port is valid (if changing HTTP port)
- Reject changes to immutable fields (data_dir, auth paths)

## 4. Integration

- `ConfigManager` owned by `HeadlessApp`
- Tools registered in tool registry
- `/config` HTTP endpoint for external management
- Restart uses `nix::sys::signal::kill(Pid::this(), Signal::SIGHUP)`

## 5. Testing

- Config get/set roundtrip
- Validation rejects invalid values
- Immutable field rejection
- Save/reload persistence
- Status output includes all expected fields

## 6. File Touchpoints

- **New:** `engine/crates/fx-config/src/manager.rs`
- **Modify:** `engine/crates/fx-cli/src/headless.rs` (add ConfigManager)
- **Modify:** `engine/crates/fx-cli/src/http_serve.rs` (add /config endpoint)
- **Modify:** `engine/crates/fx-core/src/tools/` (register tools)
- **Modify:** `engine/crates/fx-cli/src/main.rs` (SIGHUP handler)
