# Default Config Presets — Powerful Out of the Box

## Problem

Fawx ships without a default config, and users must manually configure permissions. The self-modify paths default to empty, which effectively locks down the agent. There's no workspace trust boundary, no budget caps, no sandbox settings, and no preset system in the setup wizard.

Result: Fawx feels nerfed out of the box. Users don't know what to enable.

## Design

### Three Permission Tiers

Every agent action falls into one of three tiers:

1. **Unrestricted** — Fawx does it without asking. Fast, autonomous.
2. **Proposal Required** — Fawx writes a proposal, user approves via TUI/Telegram. Powerful but supervised.
3. **Denied** — Cannot be done, period. No proposal, no override. Reserved for categorically wrong actions.

### The Denied Tier (Hardcoded, Not Configurable)

These three things are NEVER allowed regardless of config:
- `credential_exfiltrate` — Send credentials/tokens to any external destination
- `disable_logging` — Turn off the audit trail
- `weaken_proposal_gate` — Modify the proposal mechanism's own enforcement

These live in the kernel as compiled constants (like `TIER3_PATHS` today). Not in config.

### Three Presets

#### 🔥 Power User (default)

```toml
[workspace]
root = "."  # Resolved to cwd at startup; overridable

[permissions]
# Unrestricted within workspace
unrestricted = [
    "read_any",          # Read any file on the system
    "web_search",        # Search the internet
    "web_fetch",         # Fetch URLs
    "code_execute",      # Run code/shell in workspace
    "file_write",        # Create/edit files in workspace
    "git",               # Commit, branch, push
    "shell",             # Shell commands scoped to workspace
    "tool_call",         # Invoke any loaded skill/tool
    "self_modify",       # Edit own skills, prompts, loadable config
]

# Requires human approval
proposal_required = [
    "credential_change",    # Add/rotate/delete API keys
    "system_install",       # apt/brew/cargo install (system-wide)
    "network_listen",       # Bind a port, start a server
    "outbound_message",     # Send email, post publicly, message someone
    "file_delete",          # Permanent deletion
    "outside_workspace",    # Write outside workspace root
    "kernel_modify",        # Change enforcement rules
]

[budget]
max_session_cost_usd = 5.00
max_daily_cost_usd = 20.00
alert_threshold_usd = 2.00

[sandbox]
allow_network = true
allow_subprocess = true
max_execution_seconds = 300

[proposals]
auto_approve_timeout_minutes = 0  # Never auto-approve
notification_channels = ["tui"]   # Add "telegram" if configured
expiry_hours = 24
```

#### 🔒 Cautious

Same as Power User but file writes also require proposals:

```toml
[permissions]
unrestricted = [
    "read_any",
    "web_search",
    "web_fetch",
    "tool_call",
]

proposal_required = [
    "code_execute",
    "file_write",
    "git",
    "shell",
    "self_modify",
    "credential_change",
    "system_install",
    "network_listen",
    "outbound_message",
    "file_delete",
    "outside_workspace",
    "kernel_modify",
]

[budget]
max_session_cost_usd = 2.00
max_daily_cost_usd = 10.00
alert_threshold_usd = 1.00

[sandbox]
allow_network = true
allow_subprocess = false      # No subprocesses without proposal
max_execution_seconds = 60
```

#### 🧪 Experimental

Everything proposable, higher limits:

```toml
[permissions]
unrestricted = [
    "read_any",
    "web_search",
    "web_fetch",
    "code_execute",
    "file_write",
    "git",
    "shell",
    "tool_call",
    "self_modify",
    "kernel_modify",       # Can modify own kernel without proposal
]

proposal_required = [
    "credential_change",
    "system_install",
    "network_listen",
    "outbound_message",
    "file_delete",
    "outside_workspace",
]

[budget]
max_session_cost_usd = 20.00
max_daily_cost_usd = 100.00
alert_threshold_usd = 5.00

[sandbox]
allow_network = true
allow_subprocess = true
max_execution_seconds = 600
```

## Implementation

### Phase 1: Config Schema (this PR)

**File: `engine/crates/fx-config/src/lib.rs`**

Add new config sections to `FawxConfig`:

```rust
pub struct FawxConfig {
    // ... existing fields ...
    pub workspace: WorkspaceConfig,
    pub permissions: PermissionsConfig,
    pub budget: BudgetConfig,
    pub sandbox: SandboxConfig,
    pub proposals: ProposalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// Root directory for workspace operations. Defaults to cwd.
    pub root: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PermissionsConfig {
    /// Preset name: "power", "cautious", "experimental", or "custom".
    pub preset: String,
    /// Actions Fawx can perform without asking.
    pub unrestricted: Vec<String>,
    /// Actions that require human approval via proposal.
    pub proposal_required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BudgetConfig {
    /// Max cost in USD per session (0.0 = unlimited).
    pub max_session_cost_usd: f64,
    /// Max cost in USD per day (0.0 = unlimited).
    pub max_daily_cost_usd: f64,
    /// Alert threshold — warn user but don't stop.
    pub alert_threshold_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SandboxConfig {
    /// Allow network access from shell/skills.
    pub allow_network: bool,
    /// Allow subprocess spawning.
    pub allow_subprocess: bool,
    /// Kill processes after this many seconds.
    pub max_execution_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProposalConfig {
    /// Minutes before auto-approving (0 = never).
    pub auto_approve_timeout_minutes: u32,
    /// Where to send proposal notifications.
    pub notification_channels: Vec<String>,
    /// Hours before proposals expire.
    pub expiry_hours: u32,
}
```

Defaults implement the Power User preset.

### Phase 2: Setup Wizard Integration

In `engine/crates/fx-cli/src/commands/setup.rs`:

After model selection, add a permissions step:

```
🔐 Permissions preset:

  1. 🔥 Power User (recommended)
     Full workspace autonomy. Proposals for external/system actions.
     "Fawx does the work, asks before it leaves the sandbox."

  2. 🔒 Cautious
     Proposals for writes too. Good for shared machines or first-time users.
     "Fawx suggests, you approve."

  3. 🧪 Experimental
     Maximum autonomy including kernel self-modification.
     "Fawx evolves itself, you review."

Select [1-3]:
```

Write the selected preset to `config.toml`.

### Phase 3: Runtime Enforcement (future)

Wire `PermissionsConfig` into the kernel's policy evaluation:
- Check `unrestricted` before executing tool calls
- Route `proposal_required` actions through the proposal system
- Enforce `budget` limits in the LLM provider layer
- Enforce `sandbox` limits in shell execution

This is the bigger piece and should be a separate wave.

## What This PR Does

Phase 1 only:
1. Add config structs with sane Power User defaults
2. Add preset loading functions (`PermissionsConfig::power()`, `::cautious()`, `::experimental()`)
3. Ship a default `config.toml` template in the repo
4. Tests for serialization, deserialization, preset defaults
5. Setup wizard preset selection (Phase 2)

## What This PR Does NOT Do

- Runtime enforcement (Phase 3) — the config exists but isn't enforced yet
- Budget tracking — needs provider-level cost tracking infrastructure
- Sandbox enforcement — needs shell executor changes

## Files Changed

1. `engine/crates/fx-config/src/lib.rs` — New config structs + defaults
2. `engine/crates/fx-cli/src/commands/setup.rs` — Preset selection in wizard
3. `engine/crates/fx-config/src/defaults.rs` — New file: preset definitions
4. `docs/config-reference.md` — Document the new sections

## Tests

1. `power_preset_defaults_match_spec` — verify Power User defaults
2. `cautious_preset_restricts_writes` — verify Cautious blocks file_write
3. `experimental_preset_allows_kernel_modify` — verify Experimental
4. `permissions_config_round_trip` — serde serialize/deserialize
5. `custom_preset_overrides_defaults` — user can override individual permissions
6. `budget_config_zero_means_unlimited` — verify 0.0 semantics
