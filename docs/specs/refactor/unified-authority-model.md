# Spec: Unified Authority Model

## Status
Refined against current code on 2026-03-28. Implementation not started.

## Goal
Establish one canonical pre-execution authority decision for every agent action, plus one clearly separated post-execution safety channel.

Today, the engine has overlapping policy surfaces that can disagree about the same action. The result is that the loop, the user, and the developer cannot reliably predict whether a call will execute, prompt, create a proposal, or get blocked.

## Code-Validated Current Authority Chain

The live startup path is not hypothetical. In [startup.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/startup.rs#L545), the executor stack is:

`TripwireEvaluator(PermissionGateExecutor(ProposalGateExecutor(CachingExecutor(...))))`

That is not the whole story:

- Before the stack is built, [config_bridge.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/config_bridge.rs#L29) mutates the effective `SelfModifyConfig` based on `CapabilityMode` and granted permission actions.
- Inside the tool layer, [tools.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/tools.rs#L423) and [git_skill.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/git_skill.rs#L189) independently re-run self-modify checks as defense-in-depth.

So the original draft's "five overlapping control planes" is directionally right, but it understated the runtime reality. There are five primary planes, plus additional duplicated enforcement in the tool layer that must be removed or aligned during the migration.

## Corrected Problem Statement

### 1. `CapabilityMode` + permissions config

The original draft said `CapabilityMode` only changes denial style. That is not true in the current code.

In [config_bridge.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/config_bridge.rs#L29), `CapabilityMode::Capability` changes the effective self-modify policy by widening `allow_paths` when `self_modify` or `kernel_modify` is granted. In [startup.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/startup.rs#L1999), mode also changes how unknown categories behave through `default_ask`.

So today, `CapabilityMode` affects both:

- interaction style: silent denial vs prompt
- effective policy: which actions/path tiers are practically allowed

That overlap is one of the architectural problems this spec needs to eliminate.

### 2. `PermissionGateExecutor`

The original draft described a boolean `PermissionPolicy` per category. That is inaccurate.

In [permission_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_gate.rs#L30), `PermissionPolicy` is:

- `unrestricted: HashSet<String>`
- `ask_required: HashSet<String>`
- `default_ask: bool`
- `mode: CapabilityMode`

The gate is path-aware only for `write_file` / `create_file` / `edit_file`, where it uses [classify_write_domain](/Users/joseph/fawx/engine/crates/fx-core/src/self_modify.rs#L173). Everything else is still mapped through the static tool-name classifier in [permission_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_gate.rs#L391).

Two important current behaviors:

- In capability mode, unknown categories are allowed by default.
- Session-scoped approval is keyed by tool name, not by surface or path, in [permission_prompt.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_prompt.rs#L52).

That second point is a major authority leak: approving one `write_file` or `run_command` can implicitly bless later calls of the same tool even when the target surface is different.

### 3. `ProposalGateExecutor`

The original draft described proposal gate as a write-only sovereign/kernel interceptor. That is incomplete.

In [proposal_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/proposal_gate.rs#L318), proposal gate currently handles:

- write gating for `write_file`, `edit_file`, and `git_checkpoint`
- kernel-blind read blocking for `read_file`, `search_text`, and `list_directory`
- kernel-blind shell blocking for `shell`, `bash`, and `execute_command`
- active approved proposal overrides with expiry
- proposal file generation and proposal sidecar persistence

It also contains compile-time behavior through `cfg!(feature = "kernel-blind")`, which means part of the authority model is currently encoded as compiled invariants, not config.

### 4. `WriteDomain` / `PathTier` in `self_modify.rs`

This is still the cleanest starting point, but it is important to describe it precisely.

In [self_modify.rs](/Users/joseph/fawx/engine/crates/fx-core/src/self_modify.rs#L25):

- `WriteDomain` classifies write targets into `Project`, `SelfLoadable`, `KernelSource`, `Sovereign`, `External`
- `classify_write_domain(...)` is a pure-ish target classifier for write-like filesystem actions
- `classify_path(...)` produces `PathTier::{Allow, Propose, Deny}` from `SelfModifyConfig`

Those are different layers already:

- `WriteDomain` maps a path to a capability category
- `PathTier` maps a path plus mutable config to a direct/propose/deny policy

Also, when `SelfModifyConfig.enabled == false`, [classify_path](/Users/joseph/fawx/engine/crates/fx-core/src/self_modify.rs#L136) defaults to `Allow` except for `ALWAYS_PROPOSE_PATTERNS`. So the original draft's "unknown paths default to deny" is only true when self-modify enforcement is enabled.

### 5. Ripcord / tripwire

The original draft was too strong here.

In [evaluator.rs](/Users/joseph/fawx/engine/crates/fx-ripcord/src/evaluator.rs#L92), `TripwireEvaluator` does not deny or halt execution. It executes the inner call first, then:

- increments category counts
- activates the ripcord journal if a tripwire matches
- records reversible journal entries if the journal is active
- optionally notifies the user

The evaluator is a post-execution safety net, not a pre-execution policy authority. That distinction is correct and should remain.

## Concrete Conflict Scenarios The Original Draft Missed

### 1. Kernel-blind read conflict

Example: `read_file("engine/crates/fx-kernel/src/lib.rs")`

- permission gate sees `read_any`
- if `read_any` is unrestricted, permission gate allows
- proposal gate may still block because kernel-blind read enforcement is compiled in

So the same read can be both "allowed by permissions" and "blocked by proposal gate."

### 2. Shell-read conflict

Example: a shell call that runs `rg`, `grep`, `cat`, or `git diff` against a kernel-blind path.

- permission gate reasons about `shell` / `code_execute`
- proposal gate separately inspects the command text and may block it as a kernel-blind read

This is a separate policy path from ordinary path-based write gating.

### 3. `git_checkpoint` mismatch

Proposal gate explicitly treats `git_checkpoint` as a write tool in [proposal_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/proposal_gate.rs#L24).

Permission gate does not map `git_checkpoint` at all in [permission_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_gate.rs#L391), so it falls into `"unknown"`.

Current result:

- capability mode: likely allowed silently
- prompt mode: may ask because of `default_ask`
- proposal gate: may still block or create a proposal
- tool layer: [git_skill.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/git_skill.rs#L189) independently re-checks staged paths

This is an exact example of the system disagreeing with itself.

### 4. Session approval keyed too broadly

In [permission_prompt.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_prompt.rs#L52), session overrides are stored by tool name.

That means:

- approving one `write_file` can authorize later `write_file` calls to different surfaces
- approving one `run_command` can authorize later commands with different risk

Even when other gates still catch some of those calls, the approval model itself is too coarse.

### 5. Capability mode mutates path policy

In [config_bridge.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/config_bridge.rs#L29), capability mode can widen effective self-modify policy to `allow_paths = ["**"]`.

So two sessions with the same `[self_modify]` config but different permission grants can produce different `PathTier` outcomes before the gates even run.

This makes `CapabilityMode` part of policy resolution today, not just presentation.

### 6. Tool-level enforcement duplicates kernel enforcement

Built-in write tools in [tools.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/tools.rs#L423) and git checkpoint in [git_skill.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/git_skill.rs#L189) independently apply `classify_path(...)`.

That means the tool implementation can still deny or propose after the kernel wrappers already made a decision. This is exactly the kind of duplicated truth the doctrine forbids.

### 7. Latent delete surface is not unified today

The config model has `PermissionAction::FileDelete`, and permission gate maps `delete_file` / `remove_file` to `"file_delete"` in [permission_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_gate.rs#L398). Proposal gate does not do path-domain resolution for deletes.

Even if delete is not currently a first-class built-in tool in [tools.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/tools.rs#L301), the authority model already claims the surface exists. The unified model must either:

- classify delete actions fully, or
- stop advertising `file_delete` as a fully modeled authority surface until the tool path exists end-to-end

## Design Principles

### 1. One request, one authority decision

For any `(tool call, target, session state)` tuple, there must be exactly one pre-execution authority resolver that decides:

- allow
- prompt
- propose
- deny

No wrapper and no tool implementation may make an independent second policy decision about that same tuple.

### 2. Target classification is separate from policy resolution

Target classification answers:

- what surface is this action touching?
- what domain is that target in?
- what effect is being attempted?

Policy resolution answers:

- given that classified request and the current authority context, what is the verdict?

These concerns must remain separate.

### 3. Capability category and target surface are both first-class

The original draft's `ActionSurface` was close, but not sufficient on its own.

A `git_checkpoint` and a `write_file` may both touch the project, but they are not the same policy surface. Likewise, `shell`, `code_execute`, `tool_call`, and `git` are capability categories that matter even when the target path domain is the same.

So the canonical request model must preserve both:

- the requested capability category
- the classified target surface

### 4. Compiled invariants stay explicit

Kernel-blind and sovereign protections are not just config. They are compiled invariants. The resolver must take them as explicit inputs instead of letting them live as hidden side conditions inside wrappers.

### 5. Ripcord remains post-execution

Tripwire/ripcord should stay outside the pre-execution authority decision. It should consume the same canonical action taxonomy where possible, but it should not become another allow/deny/propose layer.

## Refined Proposed Architecture

### Canonical request model

Replace the original draft's `ActionSurface` with a request model that keeps capability and target classification separate:

```rust
AuthorityRequest {
    tool_name: String,
    capability: PermissionActionKind,
    effect: ActionEffect,          // Read | Write | Delete | Execute | Network
    target: Option<ActionTarget>,  // Path | Command | Network | None
}

ActionTarget::Path {
    path: PathBuf,
    domain: SurfaceDomain,         // Project | SelfLoadable | KernelSource | Sovereign | External
}
```

The exact names can change, but the shape matters:

- capability category is not inferred later from a second map
- target surface is canonicalized once
- tool name remains for attribution and compatibility only

### Authority context

`resolve_policy(surface, config)` is too narrow for the current engine. The resolver needs explicit context for everything that currently leaks through wrappers:

```rust
AuthorityContext {
    permissions: PermissionsConfig,
    self_modify: SelfModifyConfig,
    capability_mode: CapabilityMode,
    compiled_invariants: CompiledInvariantSet,
    active_proposal: Option<ApprovedProposal>,
    session_overrides: SessionAuthorityOverrides,
    working_dir: PathBuf,
}
```

This keeps hidden dependencies out of ad hoc wrappers.

### Resolution output

```rust
AuthorityResolution {
    verdict: ActionVerdict,    // Allow | Prompt | Propose | Deny
    source: AuthoritySource,   // explicit rule or invariant
    reason: String,
    details: ResolutionDetails,
}
```

Where:

- `Prompt` means interactive approval is needed
- `Propose` means proposal artifact + approval workflow is needed
- `Deny` means hard stop

The key correction to the original draft is this:

- `CapabilityMode` should influence how a restricted verdict is presented or whether interactive escalation is available
- it should not silently mutate the underlying path policy the way `config_bridge` does today

### Transitional architecture

Collapsing wrappers into one resolver is feasible, but not as a one-step delete.

The realistic path is:

1. introduce a shared resolver and shared request classifier
2. make `PermissionGateExecutor` and `ProposalGateExecutor` thin adapters over that resolver
3. remove wrapper-specific logic only after parity tests pass
4. remove tool-level defense-in-depth checks once the kernel owns the truth

This preserves behavior while paying down the overlap safely.

## Feasibility Assessment

### What is feasible

The core idea, "collapse wrappers into a single policy resolution step," is feasible.

The current code already has most of the raw ingredients:

- `WriteDomain` and `PathTier` classification in [self_modify.rs](/Users/joseph/fawx/engine/crates/fx-core/src/self_modify.rs)
- permission prompt infrastructure in [permission_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_gate.rs) and [permission_prompt.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_prompt.rs)
- proposal state + proposal artifact generation in [proposal_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/proposal_gate.rs)
- post-execution monitoring in [evaluator.rs](/Users/joseph/fawx/engine/crates/fx-ripcord/src/evaluator.rs)

### Hidden dependencies the original draft understated

1. `config_bridge.rs` currently mutates self-modify policy before the executors are built.
2. `permission_prompt.rs` stores session approvals by tool name, not by classified surface.
3. `proposal_gate.rs` owns active proposal state, expiry checks, proposal file writing, and kernel-blind feature checks.
4. `fx-tools/src/tools.rs` and `fx-tools/src/git_skill.rs` duplicate self-modify enforcement inside the tool implementations.
5. `kernel_manifest.rs` currently reports raw permission config, not effective resolved policy.
6. `permissions_to_policy(...)` in [startup.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/startup.rs#L1999) still performs category mapping and unknown-category defaults outside any unified resolver.

None of these block the refactor, but they do mean the wrapper collapse must be staged.

### Not a hard blocker: `#1639`

The original draft said this depends on `#1639` so tools can self-declare surface and effect metadata.

That is the correct end state, but it is not a hard blocker for starting this refactor.

The realistic position is:

- long term: tools self-describe their authority surface via traits
- near term: centralize the existing tool-name/argument compatibility classifier in one module so there is one temporary source of truth instead of several drifting ones

That compatibility layer should be treated as transitional debt, not the final architecture.

## Refined Deliverables

### Phase 0: Inventory and parity matrix

1. Document the real runtime chain and current decision sites.
2. Enumerate every currently shipped tool surface that participates in authority decisions, including `git_checkpoint`, `run_command`, kernel-blind reads, and tool-layer self-modify checks.
3. Build a parity matrix covering:
   - capability mode vs prompt mode
   - unrestricted vs proposal-required
   - active proposal present vs absent
   - kernel-blind enabled vs disabled
   - session approval present vs absent
4. Freeze example scenarios as tests before deleting wrappers.

### Phase 1: Canonical request classification

1. Introduce `AuthorityRequest`, `ActionEffect`, and `ActionTarget`.
2. Promote `WriteDomain` into a broader path target classifier.
3. Centralize tool-to-capability classification in one module.
4. Classify all currently shipped path-bearing actions, not just write actions.
5. Replace tool-name session overrides with request/surface-scoped overrides.

### Phase 2: Shared resolver under existing wrappers

1. Add `AuthorityContext`, `AuthorityResolution`, and `resolve_authority(...)`.
2. Make permission gate delegate to the shared resolver for prompt/deny decisions.
3. Make proposal gate delegate to the shared resolver for propose/deny decisions.
4. Move `CapabilityMode` out of policy mutation and into escalation/presentation semantics.
5. Make compiled invariants explicit inputs to the resolver instead of hidden wrapper logic.

### Phase 3: Side-effect services and attribution

1. Extract prompt handling into a dedicated approval service.
2. Extract proposal creation into a dedicated proposal service.
3. Make every restricted result return:
   - classified surface
   - capability category
   - authority source
   - human-readable reason
4. Update kernel manifest and permission surfaces to display effective policy, not just raw config lists.

### Phase 4: Collapse wrappers and remove duplicate enforcement

1. Remove `PermissionGateExecutor` logic after parity is proven.
2. Remove `ProposalGateExecutor` logic after parity is proven.
3. Remove duplicated self-modify enforcement from:
   - [tools.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/tools.rs)
   - [git_skill.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/git_skill.rs)
4. Leave ripcord/tripwire as a post-execution consumer of the canonical action taxonomy.

### Phase 5: Post-refactor follow-up

1. Align tripwire category accounting with the canonical classifier so post-execution monitoring does not drift from pre-execution authority semantics.
2. Delete any remaining compatibility maps once tool self-description lands.

## Files Implicated By This Refactor

Primary:

- [self_modify.rs](/Users/joseph/fawx/engine/crates/fx-core/src/self_modify.rs)
- [permission_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_gate.rs)
- [proposal_gate.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/proposal_gate.rs)
- [types.rs](/Users/joseph/fawx/engine/crates/fx-config/src/types.rs)
- [evaluator.rs](/Users/joseph/fawx/engine/crates/fx-ripcord/src/evaluator.rs)

Also required in practice:

- [config_bridge.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/config_bridge.rs)
- [startup.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/startup.rs)
- [permission_prompt.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/permission_prompt.rs)
- [kernel_manifest.rs](/Users/joseph/fawx/engine/crates/fx-core/src/kernel_manifest.rs)
- [tools.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/tools.rs)
- [git_skill.rs](/Users/joseph/fawx/engine/crates/fx-tools/src/git_skill.rs)
- new authority module, likely under `fx-core` or `fx-kernel`

## What Stays The Same

- sovereign and kernel-blind invariants remain explicit hard boundaries
- ripcord remains post-execution
- proposal approval still exists as a first-class workflow
- permissions config remains the operator-facing control surface

## Not In Scope

- changing ripcord rollback semantics
- changing WASM sandboxing
- redesigning the UI for prompts/proposals
- multi-user authority models
- final trait-based self-description for every tool in the same change set
