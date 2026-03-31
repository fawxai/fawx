# Step 11 Static Dispatch Audit

Issue: `#1638`
Date: 2026-03-30
Auditor: Claude Opus 4.6
Branch audited: `dev` (post-Step 10 unified authority model)

---

## 1. Executive Summary

The codebase is in significantly better shape than it was before the Step 10 authority model landed. The `#1639` tool trait refactor successfully moved cacheability, classification, and journal hints onto individual `Tool` implementations, and `#1640` eliminated provider-name dispatch. However, the kernel still contains three parallel name-to-behavior dispatch tables (`act.rs`, `authority.rs`, `kernel_blind.rs`) that classify tool calls by string name, and several supporting systems (progress reporting, cache invalidation, proposal building, bounded-local recovery) maintain their own smaller name-match tables. The tool trait methods on individual tools now *override* these central defaults, but the defaults remain as fallback paths and the authority system uses its own independent `CallSurface::from_tool_name()` table that is not derived from tool metadata at all. **Step 12 can proceed**, but two cleanup slices should happen before or alongside it: (1) collapsing `CallSurface::from_tool_name()` onto tool-declared metadata, and (2) removing the `default_tool_action_category` / `default_tool_journal_action` fallback tables in `act.rs`.

---

## 2. Findings Inventory

### F1: `CallSurface::from_tool_name()` — authority effect classification by name

- **Severity:** `must-fix-before-step12`
- **File:** `engine/crates/fx-kernel/src/authority.rs`
- **Lines:** 911-922
- **Current pattern:**
  ```rust
  fn from_tool_name(tool_name: &str) -> Self {
      match tool_name {
          "read_file" | "search_text" | "list_directory" => Self::PathRead,
          "write_file" | "create_file" | "edit_file" => Self::PathWrite,
          "delete_file" | "remove_file" => Self::PathDelete,
          "git_checkpoint" => Self::GitCheckpoint,
          "run_command" | "shell" | "bash" | "execute_command" => Self::Command,
          "web_search" | "brave_search" | "web_fetch" | "fetch_url" => Self::Network,
          _ => Self::Other,
      }
  }
  ```
- **Why it is a doctrine issue:** This is the core authority classification entry point. Every permission decision flows through this string dispatch. Tools cannot declare their own effect surface; the kernel hardcodes it. Adding a new path-writing tool means editing this match in the kernel, not implementing a trait on the tool. This is the single highest-leverage doctrine violation remaining.
- **Recommended destination:** Tool trait method (e.g., `fn call_surface(&self) -> CallSurface`) or derived from existing `action_category()` + argument inspection.
- **Estimated fix size:** `medium`
- **Dependency notes:** Benefits from `#1639` (tool trait is already landed). Could be done as a standalone slice.

---

### F2: `default_tool_action_category()` — fallback name-to-permission-category table

- **Severity:** `should-fix-soon`
- **File:** `engine/crates/fx-kernel/src/act.rs`
- **Lines:** 273-287
- **Current pattern:**
  ```rust
  fn default_tool_action_category(tool_name: &str) -> &'static str {
      match tool_name {
          "web_search" | "brave_search" => "web_search",
          "read_file" | "search_text" | "list_directory" => "read_any",
          "write_file" | "create_file" | "edit_file" => "file_write",
          "shell" | "bash" | "execute_command" => "shell",
          "git" | "git_status" | "git_diff" | "git_commit" | "git_push" => "git",
          // ... 14 arms total
          _ => "unknown",
      }
  }
  ```
- **Why it is a doctrine issue:** This was the pre-`#1639` monolithic classifier. Individual tools now override `action_category()` on the trait, but this function remains as a fallback for tools that don't override it. It duplicates truth that should live exclusively on the tool. The fallback path means a missing override can silently produce correct behavior from the wrong source.
- **Recommended destination:** Delete. If a tool doesn't declare `action_category()`, the default should be `"unknown"` (which is already the trait default), not a central lookup.
- **Estimated fix size:** `small`
- **Dependency notes:** Verify all registered tools already override `action_category()` before deleting.

---

### F3: `default_tool_journal_action()` — fallback name-to-journal-action table

- **Severity:** `should-fix-soon`
- **File:** `engine/crates/fx-kernel/src/act.rs`
- **Lines:** 289-301
- **Current pattern:**
  ```rust
  fn default_tool_journal_action(call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
      match call.name.as_str() {
          "write_file" | "create_file" | "edit_file" => file_write_action(call),
          "delete_file" | "remove_file" => file_delete_action(call),
          "git_commit" => git_commit_action(call, result),
          "git_push" => git_push_action(call, result),
          "shell" | "bash" | "execute_command" => shell_action(call, result),
          _ => None,
      }
  }
  ```
- **Why it is a doctrine issue:** Same pattern as F2. Individual tools should own their journal action via the `journal_action()` trait method. This fallback duplicates the truth.
- **Recommended destination:** Delete after verifying all relevant tools implement `journal_action()` on the trait.
- **Estimated fix size:** `small`
- **Dependency notes:** Coupled with F2 — fix both in the same slice.

---

### F4: `round_activity_descriptor()` — progress reporting by tool name

- **Severity:** `should-fix-soon`
- **File:** `engine/crates/fx-kernel/src/loop_engine/progress.rs`
- **Lines:** 356-482
- **Current pattern:** A 120+ line `match call.name.as_str()` block with 12 named arms (`web_fetch`, `web_search`, `weather`, `read_file`, `search_text`, `run_command`, `list_directory`, `kernel_manifest`, `decompose`, `current_time`, plus wildcard mutation/default). Each arm constructs a `RoundActivityDescriptor` with priority, kind, message template, and argument extraction logic.
- **Why it is a doctrine issue:** Adding a new tool with custom progress messaging requires editing this kernel function. Tools cannot describe how they should be presented during execution. The argument key extraction (`"url"`, `"query"`, `"path"`, `"command"`) is tool-specific knowledge encoded in the kernel.
- **Recommended destination:** A `progress_descriptor()` method on the `Tool` trait, or a `ProgressHint` struct returned by the tool.
- **Estimated fix size:** `medium`
- **Dependency notes:** Lower priority than F1-F3. Can be a follow-up slice.

---

### F5: `invalidate_for_side_effect()` — cache invalidation by tool name

- **Severity:** `should-fix-soon`
- **File:** `engine/crates/fx-kernel/src/caching_executor.rs`
- **Lines:** 378-391
- **Current pattern:**
  ```rust
  fn invalidate_for_side_effect(&self, tool_name: &str, arguments: &Value) {
      match tool_name {
          "write_file" => invalidate_write_file_cache(&mut cache, arguments),
          "memory_write" | "memory_delete" => invalidate_memory_cache(&mut cache, arguments),
          "run_command" => cache.flush_all_cacheable(),
          _ => {}
      }
  }
  ```
- **Why it is a doctrine issue:** Cache invalidation strategy is tool-specific knowledge (which cache keys a write invalidates, whether a command flushes everything) encoded centrally. New cacheable-but-invalidating tools must update this kernel function.
- **Recommended destination:** A `cache_invalidation_keys()` or `invalidates_cache()` method on the `Tool` trait.
- **Estimated fix size:** `small`
- **Dependency notes:** None.

---

### F6: `build_proposal_payload()` — proposal content by tool name

- **Severity:** `follow-up`
- **File:** `engine/crates/fx-kernel/src/proposal_gate.rs`
- **Lines:** 178-186
- **Current pattern:**
  ```rust
  fn build_proposal_payload(call: &ToolCall, path: &str, working_dir: &Path) -> Result<String, String> {
      match call.name.as_str() {
          "edit_file" => build_edit_proposal_payload(call, path, working_dir),
          _ => build_write_proposal_payload(call, path, working_dir),
      }
  }
  ```
- **Why it is a doctrine issue:** The proposal gate distinguishes edit from write by tool name to produce different proposal payloads (diff-based vs. full content). This is tool-specific presentation knowledge in the kernel.
- **Recommended destination:** A `proposal_payload()` method on tools that support proposals, or an `edit_metadata` field on the authority request.
- **Estimated fix size:** `small`
- **Dependency notes:** Part of the unified authority model (Step 10) follow-through. Lower priority.

---

### F7: `bounded_local_recovery_focus_from_calls()` — name match for recovery focus

- **Severity:** `follow-up`
- **File:** `engine/crates/fx-kernel/src/loop_engine/bounded_local.rs`
- **Lines:** 535-549
- **Current pattern:**
  ```rust
  for call in calls {
      if !matches!(call.name.as_str(), "edit_file" | "write_file") {
          continue;
      }
      // extract path for focus
  }
  ```
- **Why it is a doctrine issue:** The loop engine decides which tools produce "focus files" for bounded-local recovery by matching on tool names. This is a behavioral classification that should be on the tool (e.g., `fn produces_file_focus(&self) -> bool`).
- **Recommended destination:** Tool trait method or derived from `cacheability == SideEffect` + path-bearing argument.
- **Estimated fix size:** `small`
- **Dependency notes:** None.

---

### F8: Shell command classification tables

- **Severity:** `acceptable-for-now`
- **File:** `engine/crates/fx-tools/src/tools/shell.rs`
- **Lines:** 360-420
- **Current pattern:** Static match tables for `is_observational_program_and_args()`, `is_observational_git_command()`, and `is_observational_cargo_command()`. Maps 30+ program names and subcommands to observation/mutation classification.
- **Why it is borderline:** This is classification of *external programs*, not of Fawx components. The shell tool owns this classification because it is the only tool that runs arbitrary external commands. There is no external "owner" for `grep` or `git status` to push this classification to. The knowledge lives on the shell tool implementation, which is the correct owner under doctrine.
- **Why it is listed:** The tables are large and could drift if new observational commands are added to the system. However, this is a content maintenance issue, not a structural doctrine issue.
- **Recommended destination:** Stays where it is. Consider extracting to a data file or const array if the list grows substantially.
- **Estimated fix size:** N/A

---

### F9: `kernel_blind.rs` — static path and command prefix tables

- **Severity:** `acceptable-for-now`
- **File:** `engine/crates/fx-kernel/src/kernel_blind.rs`
- **Lines:** 1-15
- **Current pattern:** Static `const` arrays for `KERNEL_BLIND_PATH_PREFIXES`, `READ_COMMAND_PREFIXES`, `SEARCH_COMMAND_PREFIXES`, `GIT_COMMAND_PREFIXES`, and `RE_COMMAND_PREFIXES`.
- **Why it is borderline:** Kernel-blind is a compiled security invariant. The paths that are blind are not characteristics of individual tools — they are properties of the *filesystem layout* enforced by the kernel's security boundary. The command prefixes detect when shell commands attempt to read blind paths via external programs.
- **Why it is listed:** The command prefix tables duplicate knowledge about what constitutes a "read command" or "search command" (similar to shell.rs's observational tables). However, the purpose is different: kernel_blind cares about *any* path-revealing command, not about mutation classification.
- **Recommended destination:** Stays in the kernel. The path list is an invariant; the command prefix list could potentially be consolidated with shell.rs's observational classification, but the risk/reward is low.
- **Estimated fix size:** N/A

---

### F10: `self_modify.rs` path pattern registries

- **Severity:** `acceptable-for-now`
- **File:** `engine/crates/fx-core/src/self_modify.rs`
- **Lines:** 46-94
- **Current pattern:** Static `const` arrays for `SELF_LOADABLE_PATH_PATTERNS`, `KERNEL_SOURCE_PATH_PATTERNS`, `SOVEREIGN_WRITE_PATH_PATTERNS`, `DEFAULT_DENY_PATHS`, and `ALWAYS_PROPOSE_PATTERNS`.
- **Why it is borderline:** These are path classification rules, not component-level metadata. They classify the *filesystem* into security domains. No individual tool "owns" the fact that `fx-kernel/` is kernel source — that is a system-level invariant. The patterns serve the same purpose as kernel-blind paths: compiled security boundaries.
- **Recommended destination:** Stays in `self_modify.rs`. These are architectural invariants, not component metadata.
- **Estimated fix size:** N/A

---

### F11: `DEFAULT_DENY_PATHS` duplication across crates

- **Severity:** `should-fix-soon`
- **File(s):**
  - `engine/crates/fx-core/src/self_modify.rs:77`
  - `engine/crates/fx-config/src/defaults.rs:14`
- **Current pattern:** The same deny paths (`.git/**`, `*.key`, `*.pem`, `credentials.*`) are defined in both crates with a comment warning: "These patterns are duplicated from `fx_core::self_modify::DEFAULT_DENY_PATHS` to keep fx-config independent of fx-core."
- **Why it is a doctrine issue:** Duplicate truth across layers. The comment itself acknowledges the drift risk. If one is updated without the other, deny enforcement diverges between config defaults and runtime enforcement.
- **Recommended destination:** Either make `fx-config` depend on `fx-core` for this constant, or extract the shared constant into a tiny shared crate/module that both depend on.
- **Estimated fix size:** `small`
- **Dependency notes:** Independent of other findings.

---

### F12: `capability_denied_result()` — denial messages by capability string

- **Severity:** `follow-up`
- **File:** `engine/crates/fx-kernel/src/permission_gate.rs`
- **Lines:** 388-406
- **Current pattern:** User-facing denial messages dispatched by matching on `decision.reason.as_str()` and `decision.request.capability.as_str()`.
- **Why it is a doctrine issue:** Denial message text is encoded centrally in the permission gate, not owned by the capability or authority source that produced the denial. Changes to denial messaging require editing this kernel function.
- **Recommended destination:** `AuthorityDecision` or `AuthorityResolution` should carry a structured denial reason with a human-readable message, set by the resolver at decision time.
- **Estimated fix size:** `small`
- **Dependency notes:** Part of unified authority model follow-through.

---

### F13: `WriteDomain::permission_category()` — domain-to-string mapping

- **Severity:** `acceptable-for-now`
- **File:** `engine/crates/fx-core/src/self_modify.rs`
- **Lines:** 33-42
- **Current pattern:**
  ```rust
  pub const fn permission_category(self) -> &'static str {
      match self {
          Self::Project => "file_write",
          Self::SelfLoadable => "self_modify",
          Self::KernelSource | Self::Sovereign => "kernel_modify",
          Self::External => "outside_workspace",
      }
  }
  ```
- **Why it is borderline:** This maps write domains to permission category strings. The mapping is small, stable, and the `WriteDomain` enum is the canonical owner of this knowledge. The strings themselves (`"file_write"`, `"self_modify"`, etc.) are the permission vocabulary shared with config and authority. This is a reasonable place for the mapping to live.
- **Recommended destination:** Stays. Could eventually become `PermissionActionKind` enum variants instead of strings, but that is a Step 10 deliverable, not a drift issue.
- **Estimated fix size:** N/A

---

### F14: `git_skill.rs` self-modify check in `execute_merge()`

- **Severity:** `follow-up`
- **File:** `engine/crates/fx-tools/src/git_skill.rs`
- **Lines:** 222-227
- **Current pattern:**
  ```rust
  match &self.self_modify {
      Some(config) if config.enabled => {}
      _ => {
          return Err("git_merge requires self-modification to be enabled".to_string());
      }
  }
  ```
- **Why it is a doctrine issue:** The git skill duplicates authority enforcement that the kernel should own. After the unified authority model is complete, tools should not independently gate on `SelfModifyConfig`.
- **Recommended destination:** Remove once the authority resolver handles this check pre-execution.
- **Estimated fix size:** `small`
- **Dependency notes:** Blocked on unified authority model Phase 4 (collapse wrappers, remove duplicate enforcement).

---

### F15: Skill-level `execute_tool()` string dispatch (session, cron, git skills)

- **Severity:** `acceptable-for-now`
- **File(s):**
  - `engine/crates/fx-tools/src/session_tools.rs:39-48`
  - `engine/crates/fx-tools/src/cron_skill.rs:46-58`
  - `engine/crates/fx-tools/src/git_skill.rs:370-383`
- **Current pattern:** Each skill uses `match tool_name { "skill_tool_a" => ..., "skill_tool_b" => ... }` to route calls to handler methods.
- **Why it is borderline:** These are internal dispatch within a skill, not cross-layer classification. The `Skill` trait exposes multiple tools from a single implementation. The skill is the owner of its own tool routing. This is local coordination, not doctrine drift — the match lives in the component that owns the tools.
- **Recommended destination:** Stays. If skills are decomposed into individual `Tool` impls in the future, this dispatch disappears naturally.
- **Estimated fix size:** N/A

---

### F16: Ripcord evaluator argument extraction

- **Severity:** `follow-up`
- **File:** `engine/crates/fx-ripcord/src/evaluator.rs`
- **Lines:** 137-147
- **Current pattern:**
  ```rust
  fn extract_path(arguments: &Value) -> Option<String> {
      string_arg(arguments, "path").or_else(|| string_arg(arguments, "file_path"))
  }
  fn extract_command(arguments: &Value) -> Option<String> {
      string_arg(arguments, "command")
  }
  ```
- **Why it is a doctrine issue:** The ripcord evaluator knows tool-specific argument key names (`"path"`, `"file_path"`, `"command"`). If a tool changes its argument schema, the evaluator silently stops extracting data for journaling.
- **Recommended destination:** Tools should provide structured journal metadata via `journal_action()` on the trait, which already exists. The evaluator should consume that metadata instead of re-parsing arguments.
- **Estimated fix size:** `small`
- **Dependency notes:** Partially addressed by `#1639`. Verify whether the evaluator already delegates to `tool.journal_action()` or still falls back to argument extraction.

---

## 3. Acceptable Centralization

The following patterns may look like doctrine violations but are reasonable for now:

### Shell command classification (F8)
The shell tool's `is_observational_program_and_args()` maintains a static table of 30+ external programs and subcommands. This is classification of *external programs the shell invokes*, not of Fawx components. There is no "owner" for `grep` or `git status` within the Fawx architecture to push this classification to. The shell tool is the correct owner. This is a content maintenance concern, not a structural one.

### Kernel-blind path and command tables (F9)
These are compiled security invariants that define the kernel's own protection boundary. The kernel is the correct owner of the knowledge about which of its own paths are blind. The command prefix tables are a practical detection heuristic for shell-mediated reads of blind paths.

### Self-modify path pattern registries (F10)
`SELF_LOADABLE_PATH_PATTERNS`, `KERNEL_SOURCE_PATH_PATTERNS`, and `SOVEREIGN_WRITE_PATH_PATTERNS` classify the *filesystem layout* into security domains. These are system-level invariants, not component metadata. The classification belongs to the security enforcement layer (`self_modify.rs`), not to individual tools.

### `WriteDomain::permission_category()` (F13)
A small, stable mapping from write domain enum to permission category string. The `WriteDomain` type is the canonical owner of this mapping. Will naturally evolve into typed enum variants when the unified authority model reaches maturity.

### Skill-level internal dispatch (F15)
Skills that expose multiple tools route calls internally by name. This is local coordination within the owning component, not cross-layer classification. The match lives where the tools are defined.

---

## 4. Recommended Execution Plan

### Slice A: Collapse authority classification onto tool metadata
- **Goal:** Eliminate `CallSurface::from_tool_name()` by deriving the call surface from tool-declared metadata.
- **Files/crates:** `fx-kernel/src/authority.rs`, `fx-tools/src/tool_trait.rs`, individual tool impls in `fx-tools/src/tools/`
- **Why isolated:** This is a pure refactor of one classification path. The authority resolver already has access to the tool executor; it needs to query tool metadata instead of matching on the name string.
- **Timing:** Before Step 12. This is the highest-leverage single change.

### Slice B: Remove `act.rs` fallback tables
- **Goal:** Delete `default_tool_action_category()` and `default_tool_journal_action()`. Verify all registered tools already provide these methods on the trait.
- **Files/crates:** `fx-kernel/src/act.rs`, verify `fx-tools/src/tools/*.rs` all override `action_category()` and `journal_action()`.
- **Why isolated:** Pure deletion after verification. No new code needed — just confirm the trait implementations exist and remove the dead fallback path.
- **Timing:** Before or during Step 12. Quick win.

### Slice C: Deduplicate DEFAULT_DENY_PATHS
- **Goal:** Eliminate the duplicated deny paths between `fx-core` and `fx-config`.
- **Files/crates:** `fx-core/src/self_modify.rs`, `fx-config/src/defaults.rs`
- **Why isolated:** Small, mechanical fix. Either add a dependency or extract a shared constant.
- **Timing:** Before Step 12. Trivial but removes a documented drift risk.

### Slice D: Move cache invalidation and progress descriptors to tool trait
- **Goal:** Add `cache_invalidation()` and `progress_descriptor()` methods to the `Tool` trait. Migrate `invalidate_for_side_effect()` and `round_activity_descriptor()` to delegate to tool metadata.
- **Files/crates:** `fx-tools/src/tool_trait.rs`, `fx-kernel/src/caching_executor.rs`, `fx-kernel/src/loop_engine/progress.rs`, individual tool impls.
- **Why isolated:** These are two independent but structurally similar migrations. Both replace kernel-side name dispatch with tool-side trait methods.
- **Timing:** During or after Step 12. Lower priority than Slices A-C.

### Slice E: Authority model follow-through (denial messages, proposal payloads, tool-level enforcement)
- **Goal:** Clean up remaining authority model debt: structured denial reasons (F12), proposal payload ownership (F6), and remove tool-level self-modify checks (F14).
- **Files/crates:** `fx-kernel/src/permission_gate.rs`, `fx-kernel/src/proposal_gate.rs`, `fx-tools/src/git_skill.rs`
- **Why isolated:** These are all part of the unified authority model Phase 3-4 deliverables. They depend on the authority resolver being fully in place.
- **Timing:** After Step 12. Part of the authority model maturation, not a blocker.

---

## 5. Suggested Issue / PR Sequence

1. **Slice C** — Deduplicate `DEFAULT_DENY_PATHS` (trivial, independent, removes a documented risk)
2. **Slice B** — Remove `act.rs` fallback tables (small, verifiable, removes the most visible legacy dispatch)
3. **Slice A** — Collapse `CallSurface::from_tool_name()` onto tool metadata (medium, highest-leverage doctrine fix)
4. **Slice D** — Cache invalidation + progress descriptors to trait (medium, two independent sub-PRs possible)
5. **Slice E** — Authority model follow-through (small-medium, depends on authority model maturity)

Slices C, B, and A should be completed before or alongside Step 12. Slices D and E can follow.

The smallest sequence that meaningfully reduces doctrine drift is **C + B + A** (three PRs, removing the three most impactful static dispatch patterns). Slices D and E are valuable follow-ups but not blockers.
