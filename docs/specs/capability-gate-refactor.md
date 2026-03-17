# Spec: CapabilityGateExecutor Refactor (Phase 1)

**PR target:** `dev`  
**Branch:** `feat/capability-gate`  
**References:** `docs/architecture/ax-kernel-security-model.md`

---

## Goal

Replace per-action consent prompts with silent capability-based enforcement. Actions are either allowed (execute freely) or denied (immediate structured error). No pausing, no SSE prompts, no 300s timeouts.

Keep the prompt machinery as opt-in for users who explicitly want it.

---

## Changes

### 1. fx-config: New presets + capability mode flag

**File:** `engine/crates/fx-config/src/lib.rs`

**a) Add `CapabilityMode` to `PermissionsConfig`:**
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityMode {
    /// Default: denied actions are silently blocked with structured error.
    Capability,
    /// Opt-in: denied actions trigger interactive prompts (legacy behavior).
    Prompt,
}

impl Default for CapabilityMode {
    fn default() -> Self {
        Self::Capability
    }
}
```

**b) Add `mode` field to `PermissionsConfig`:**
```rust
pub struct PermissionsConfig {
    pub preset: PermissionPreset,
    pub mode: CapabilityMode,  // NEW — defaults to Capability
    pub unrestricted: Vec<PermissionAction>,
    pub proposal_required: Vec<PermissionAction>,
}
```

**c) Rename presets (keep old names as aliases for backwards compat):**
- `Power` → `Open` (alias: `Power`)
- `Cautious` → `Standard` (alias: `Cautious`)
- `Experimental` → `Restricted` ... wait, that's backwards.

Actually, keep the existing preset names AND add new ones as aliases. The preset content stays the same — what changes is the enforcement mode. The presets already define which actions are unrestricted vs proposal_required. Under `Capability` mode, `proposal_required` becomes `denied` (silent block). Under `Prompt` mode, `proposal_required` triggers prompts (legacy).

So: **keep existing presets as-is.** Add `mode: CapabilityMode` field. Default to `Capability`.

Add new preset constructors:
```rust
/// Open — everything allowed except privilege escalation.
pub fn open() -> Self {
    Self {
        preset: PermissionPreset::Experimental,
        mode: CapabilityMode::Capability,
        ..Self::experimental()
    }
}

/// Standard — developer workflow, credential/system changes blocked.
pub fn standard() -> Self {
    Self {
        preset: PermissionPreset::Power,
        mode: CapabilityMode::Capability,
        ..Self::power()
    }
}

/// Restricted — read-heavy, most writes blocked.
pub fn restricted() -> Self {
    Self {
        preset: PermissionPreset::Cautious,
        mode: CapabilityMode::Capability,
        ..Self::cautious()
    }
}
```

**d) Update `Default` impl:** default to `standard()` (was `power()`).

### 2. fx-kernel: CapabilityMode in PermissionPolicy

**File:** `engine/crates/fx-kernel/src/permission_gate.rs`

**a) Add mode to PermissionPolicy:**
```rust
pub struct PermissionPolicy {
    pub unrestricted: HashSet<String>,
    pub ask_required: HashSet<String>,
    pub default_ask: bool,
    pub mode: CapabilityMode,  // NEW
}
```

Update `allow_all()` to set `mode: CapabilityMode::Capability`.

**b) Change `check_permission` behavior based on mode:**

```rust
async fn check_permission(
    &self,
    call: &ToolCall,
    cancel: Option<&CancellationToken>,
) -> PermissionCheck {
    let category = tool_to_action_category(&call.name);

    if !self.permissions.requires_asking(category) {
        return PermissionCheck::Allowed;
    }

    if self.prompt_state.is_session_allowed(&call.name) {
        return PermissionCheck::Allowed;
    }

    // NEW: In capability mode, silently deny instead of prompting
    match self.permissions.mode {
        CapabilityMode::Capability => {
            PermissionCheck::Denied(capability_denied_result(call, category))
        }
        CapabilityMode::Prompt => {
            self.ask_permission(call, category, cancel).await
        }
    }
}
```

**c) Add structured denial for capability mode:**
```rust
fn capability_denied_result(call: &ToolCall, category: &str) -> ToolResult {
    let suggestion = match category {
        "network_listen" | "outbound_message" => 
            "This capability is not available in this session. Request a capability grant or use an alternative approach.",
        "credential_change" | "system_install" | "kernel_modify" =>
            "This action requires elevated privileges not available in this session.",
        "file_delete" | "outside_workspace" =>
            "This action is outside the current session's capability space.",
        _ =>
            "This action is not permitted in the current session configuration.",
    };
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: false,
        output: format!(
            "CAPABILITY_DENIED: Action category '{category}' is not in this session's capability space. {suggestion}"
        ),
    }
}
```

**d) Update tests:** Add tests for capability mode behavior:
- `capability_mode_silently_denies_restricted_tool` — no prompt emitted, immediate denial
- `prompt_mode_still_prompts` — legacy behavior works when mode is Prompt
- `capability_mode_allows_unrestricted_tool` — unrestricted tools unaffected
- `capability_mode_session_override_still_works` — session overrides still bypass denial

### 3. fx-cli/startup.rs: Wire capability mode

**File:** `engine/crates/fx-cli/src/startup.rs`

**a) Pass mode through `permissions_to_policy`:**
```rust
fn permissions_to_policy(config: &fx_config::PermissionsConfig) -> PermissionPolicy {
    let unrestricted = config.unrestricted.iter().map(|a| a.as_str().to_string()).collect();
    let ask_required = config.proposal_required.iter().map(|a| a.as_str().to_string()).collect();
    let has_ask_entries = !config.proposal_required.is_empty();
    PermissionPolicy {
        unrestricted,
        ask_required,
        default_ask: has_ask_entries,
        mode: config.mode,  // NEW
    }
}
```

**b) In `CapabilityMode::Capability`, the prompt_state and stream_callback are still created (for potential runtime mode switch), but the gate won't use them unless mode is Prompt.**

### 4. fx-api: Update permissions handler

**File:** `engine/crates/fx-api/src/handlers/permissions.rs`

**a) Include `mode` in `PermissionsResponse`:**
```rust
pub struct PermissionsResponse {
    pub preset: String,
    pub mode: String,  // NEW: "capability" or "prompt"
    pub permissions: Vec<PermissionEntry>,
    pub available_presets: Vec<String>,
}
```

**b) Include `mode` in PATCH request:**
```rust
pub struct PermissionsPatchRequest {
    pub preset: Option<String>,
    pub mode: Option<String>,  // NEW
    pub changes: Option<Vec<PermissionChange>>,
}
```

**c) Update level display:** When mode is `Capability`, actions in `proposal_required` should display level as `"denied"` instead of `"ask"`.

### 5. SSE: Keep permission_prompt event type

No changes to SSE. The event type stays for `Prompt` mode. In `Capability` mode it's just never emitted.

### 6. Permission prompts handler: Keep as-is

`fx-api/handlers/permission_prompts.rs` stays unchanged. It's the endpoint for resolving prompts in `Prompt` mode. In `Capability` mode, no prompts are created, so the endpoint is simply unused.

---

## What NOT to change

- **Do NOT rename files or types.** `PermissionGateExecutor` keeps its name. Adding a type alias `CapabilityGateExecutor = PermissionGateExecutor<T>` is fine but renaming the struct creates churn.
- **Do NOT remove prompt code.** It's opt-in now, not dead.
- **Do NOT change the executor chain order.** PermissionGate → ProposalGate → CachingExecutor → SkillRegistry stays.
- **Do NOT touch fx-api tests that construct PermissionPromptState.** They still need it.

---

## Config backward compat

When `mode` is absent from config.toml, it deserializes to `Capability` (the default). Existing users on Power preset get the same behavior (Power was already mostly unrestricted). Existing users on Cautious who relied on prompts will notice the change — their `proposal_required` actions now silently fail instead of prompting. They can add `mode = "prompt"` to restore old behavior.

This is an intentional breaking change. The old default was broken (300s timeout, modal never appeared).

---

## Test matrix

| Mode | Action category | Expected |
|------|----------------|----------|
| Capability | unrestricted | Allowed |
| Capability | ask_required | Silently denied with structured error |
| Capability | unknown (default_ask=true) | Silently denied |
| Capability | session-overridden | Allowed |
| Prompt | unrestricted | Allowed |
| Prompt | ask_required | Prompt emitted, waits for response |
| Prompt | denied by user | Denied |
| Prompt | allowed by user | Allowed |

---

## Build verification

```bash
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Both must pass with zero errors/warnings before committing.
