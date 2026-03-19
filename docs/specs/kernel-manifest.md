# Kernel Manifest — Spec

Date: 2026-03-18
Status: Implementation-ready

## Problem

Fawx agents lack structured self-knowledge about what the kernel provides.
The agent has tools to read files and query `fawx_status`, but no way to
introspect its own permission boundaries, budget limits, tripwire config,
sandbox rules, or available capability primitives. This causes:

1. **Wasted tool calls** — agent discovers boundaries by hitting them
2. **Retry spirals** — denied actions trigger retries instead of replanning
3. **Redundant proposals** — agent proposes building features that already exist
   (see PR #1486: kernel capability roadmap that reinvented 5 shipped features)
4. **Poor planning** — no preflight validation against actual constraints

## Solution

A `KernelManifest` struct assembled at session start from existing in-memory
state, exposed as both a tool (`kernel_manifest`) and optionally injected
into the system prompt as structured context.

## Layer Alignment

The manifest deliberately surfaces **Layer 1** (capabilities — visible by
design) and **Layer 3** (hard boundaries — visible when hit) information.

It **excludes Layer 2** (tripwire/ripcord) information entirely. The AX
security model's core invariant is: "The agent does not know tripwires
exist, does not know when one is crossed, and does not know its actions
are being journaled." Exposing tripwire config would collapse Layer 2
into Layer 1, giving adversarial agents an evasion roadmap and causing
well-behaved agents to hedge near boundaries (degrading output quality).

This exclusion is non-negotiable and must be preserved if the manifest
is extended in the future.

## Design

### KernelManifest struct

Located in `fx-core/src/kernel_manifest.rs`:

```rust
/// Machine-readable snapshot of the kernel's current configuration,
/// capabilities, and boundaries. Designed for agent self-introspection
/// to enable informed planning without trial-and-error discovery.
///
/// Surfaces Layer 1 (capabilities) and Layer 3 (hard boundaries).
/// Deliberately excludes Layer 2 (tripwire/ripcord) per the AX
/// invisibility invariant.
#[derive(Debug, Clone, Serialize)]
pub struct KernelManifest {
    /// Fawx version.
    pub version: String,
    /// Active permission preset name (e.g. "open", "standard", "restricted").
    pub preset: Option<String>,
    /// Active model and provider.
    pub model: ModelInfo,
    /// Permission boundaries.
    pub permissions: PermissionManifest,
    /// Budget limits.
    pub budget: BudgetManifest,
    /// Sandbox enforcement rules.
    pub sandbox: SandboxManifest,
    /// Self-modification policy.
    pub self_modify: SelfModifyManifest,
    /// Available tools grouped by skill.
    pub tools: Vec<SkillManifest>,
    /// Working directory and filesystem boundaries.
    pub workspace: WorkspaceManifest,
}
```

### Sub-structs

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub active_model: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionManifest {
    /// Permission mode: "capability" (default) or "prompt" (legacy).
    pub mode: String,
    /// Categories that execute freely.
    pub unrestricted: Vec<String>,
    /// Categories that are denied (capability mode) or require approval (prompt mode).
    pub restricted: Vec<String>,
    /// What happens to unmapped tools: "allow" or "deny".
    pub default_policy: String,
    /// Whether the agent can request capability escalation via request_capability tool.
    pub can_request_capabilities: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetManifest {
    pub max_llm_calls: u32,
    pub max_tool_invocations: u32,
    pub max_tokens: u64,
    pub max_wall_time_seconds: u64,
    pub max_retries_per_tool: u32,
    pub max_fan_out: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SandboxManifest {
    pub allow_network: bool,
    pub allow_subprocess: bool,
    pub max_execution_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelfModifyManifest {
    pub enabled: bool,
    pub allow_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillManifest {
    pub name: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceManifest {
    pub working_dir: String,
    /// Writable root paths (from self-modify allow_paths or working_dir).
    pub writable_roots: Vec<String>,
}
```

### Assembly

A `build_kernel_manifest()` function takes references to the existing
runtime objects and assembles the manifest. No new data collection —
everything is already in memory:

| Field | Source |
|-------|--------|
| `version` | `RuntimeInfo.version` |
| `model` | `RuntimeInfo.active_model`, `RuntimeInfo.provider` |
| `permissions` | `PermissionPolicy` (unrestricted, ask_required, mode) |
| `budget` | `BudgetConfig` |
| `preset` | `PermissionsConfig.preset` |
| `sandbox` | `SandboxConfig` |
| `self_modify` | `SelfModifyConfig` |
| `tools` | `RuntimeInfo.skills` |
| `workspace` | `working_dir`, self-modify allow_paths |

### Tool: `kernel_manifest`

Added to `BuiltinToolsSkill` in `fx-tools/src/tools.rs`:

```rust
ToolDefinition {
    name: "kernel_manifest".to_string(),
    description: "Get a structured description of the kernel's current configuration, \
        permissions, budget limits, sandbox rules, and available tools. Use this at the \
        start of complex tasks to understand your capabilities and constraints before \
        planning.".to_string(),
    parameters: serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    }),
}
```

Returns the manifest as pretty-printed JSON.

### System prompt injection (optional, Phase 2)

After the tool works, optionally inject a condensed version into the system
prompt at session start. This gives the agent immediate awareness without
needing to call a tool. The condensed version would be ~20-30 lines of
structured text, not the full JSON.

## File changes

| File | Change |
|------|--------|
| `engine/crates/fx-core/src/kernel_manifest.rs` | **NEW** — all structs + `build_kernel_manifest()` |
| `engine/crates/fx-core/src/lib.rs` | Add `pub mod kernel_manifest;` |
| `engine/crates/fx-tools/src/tools.rs` | Add `kernel_manifest` tool handler |
| `engine/crates/fx-cli/src/startup.rs` | Build manifest and pass to tool executor |

## Tests

1. `build_kernel_manifest_includes_version` — verify version field populated
2. `build_kernel_manifest_reflects_permission_mode` — capability vs prompt
3. `build_kernel_manifest_lists_restricted_categories` — ask_required mapped
4. `build_kernel_manifest_includes_budget_limits` — all budget fields present
5. `build_kernel_manifest_includes_sandbox` — network, subprocess, timeout
6. `build_kernel_manifest_includes_tools` — skill names and tool lists
7. `build_kernel_manifest_serializes_to_json` — serde roundtrip
8. `kernel_manifest_tool_returns_json` — tool execution returns valid JSON
9. `empty_permission_policy_shows_no_restrictions` — allow_all policy
10. `build_kernel_manifest_excludes_tripwire_state` — verify NO tripwire info leaks
11. `build_kernel_manifest_includes_preset_name` — preset field populated
12. `build_kernel_manifest_includes_escalation_flag` — can_request_capabilities

## Integration test (Joe)

After merge: ask Fawx "what can you do?" or "what are your limits?" — it
should call `kernel_manifest` and give an accurate, grounded answer instead
of hallucinating capabilities.

## Estimated scope

~250-350 lines including tests. Single PR. No dependency changes needed —
fx-core already has serde, and the tool executor already has access to all
the source data.

## Why this matters

This is the smallest change that produces the largest improvement in agent
planning quality. Every tool call that gets denied because the agent didn't
know the boundary is a wasted LLM round-trip. Every retry spiral that could
have been avoided by checking budget first is wasted compute. And every
feature proposal that reinvents existing infrastructure is wasted human
attention.

The agent that wrote PR #1486 needed exactly this tool.
