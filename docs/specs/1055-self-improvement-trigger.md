# #1055 — Self-Improvement Trigger: Signal → Fix Loop

**Status:** Implementation Spec  
**Date:** 2026-03-03  
**Prerequisites:** Signal analysis (✅ fx-analysis), Proposal system (✅ fx-propose), Git tools (✅ GitSkill)  
**Crate scope:** New `fx-improve` crate + wiring in `fx-cli`

---

## 1. Problem Statement

Fawx has all the pieces for a self-improvement loop, but they're disconnected:

- **fx-analysis** detects patterns from signals (recurring friction, tool failures, performance trends) and returns `Vec<AnalysisFinding>` with confidence + evidence + suggested actions
- **fx-propose** writes structured proposals to `~/.fawx/proposals/` for human review
- **GitSkill** can create branches, checkpoint changes, and manage diffs
- **LoopEngine** emits signals every cycle but never acts on accumulated patterns

The gap: nothing connects "I keep seeing this friction pattern" to "here's a fix proposal." The human has to notice patterns manually, then ask Fawx to fix them.

### What this feature does

Closes the loop: `signals → analysis → improvement plan → proposal/issue → human approval`

### What this feature does NOT do

- Does not auto-merge (human gate always required)
- Does not replace the Anticipation Loop (#1003) — this is inward-facing (self-improvement), not user-facing (proactive assistance)
- Does not do LoRA tuning (#1004) — this changes code, not model weights
- Does not run autonomously in the background (yet) — triggered post-session or manually

---

## 2. Existing Infrastructure (verified against staging HEAD)

| Component | Location | What it provides |
|-----------|----------|-----------------|
| `SignalStore` | `fx-memory/src/signal_store.rs` | `load_all() -> Vec<(String, Signal)>`, `list_all_sessions()`, `query(SignalQuery)`, `cleanup_old_signals()` |
| `AnalysisEngine` | `fx-analysis/src/engine.rs` | `analyze(provider) -> Vec<AnalysisFinding>` — LLM-powered pattern detection over signals |
| `AnalysisFinding` | `fx-analysis/src/findings.rs` | `pattern_name`, `description`, `confidence` (High/Medium/Low), `evidence: Vec<SignalEvidence>`, `suggested_action: Option<String>` |
| `ProposalWriter` | `fx-propose/src/lib.rs` | `write(Proposal) -> PathBuf` — writes structured markdown proposals to `~/.fawx/proposals/` |
| `Proposal` | `fx-propose/src/lib.rs` | `title`, `description`, `target_path`, `proposed_content`, `risk`, `timestamp` |
| `GitSkill` | `fx-tools/src/git_skill.rs` | `git_branch_create`, `git_checkpoint`, `git_diff`, `git_branch_switch`, `git_revert` |
| `SelfModifyConfig` | `fx-tools/src/git_skill.rs` | `allow`/`propose`/`deny` tier enforcement for file paths |

---

## 3. New Files

| File | Purpose |
|------|---------|
| `engine/crates/fx-improve/Cargo.toml` | New crate: depends on `fx-analysis`, `fx-propose`, `fx-llm`, `fx-memory`, `fx-core` |
| `engine/crates/fx-improve/src/lib.rs` | Re-exports |
| `engine/crates/fx-improve/src/detector.rs` | `ImprovementDetector` — filters findings by threshold, deduplicates against known issues |
| `engine/crates/fx-improve/src/planner.rs` | `ImprovementPlanner` — generates fix plans from filtered findings using LLM |
| `engine/crates/fx-improve/src/executor.rs` | `ImprovementExecutor` — creates proposals or files issues from plans |
| `engine/crates/fx-improve/src/config.rs` | `ImprovementConfig` — thresholds, cooldowns, output mode |
| `engine/crates/fx-improve/src/tests.rs` | Unit + integration tests |

## Modified Files

| File | Change |
|------|--------|
| `engine/Cargo.toml` | Add `fx-improve` workspace member |
| `engine/crates/fx-cli/src/tui.rs` | Add `/improve` command, wire post-session trigger |
| `engine/crates/fx-cli/Cargo.toml` | Add `fx-improve` dependency |

---

## 4. API Design

### 4.1 ImprovementConfig

```rust
// fx-improve/src/config.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementConfig {
    /// Minimum confidence level for a finding to be actionable.
    /// Default: High
    pub min_confidence: Confidence,
    /// Minimum number of evidence signals for a finding to be actionable.
    /// Default: 3
    pub min_evidence_count: usize,
    /// Output mode for improvement actions.
    /// Default: ProposalOnly
    pub output_mode: OutputMode,
    /// Cooldown: minimum hours between improvement runs.
    /// Default: 24
    pub cooldown_hours: u32,
    /// Maximum improvements per run.
    /// Default: 3
    pub max_improvements_per_run: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutputMode {
    /// Write proposals to ~/.fawx/proposals/ only.
    ProposalOnly,
    /// Write proposals AND create a branch with the fix applied.
    ProposalWithBranch,
    /// Dry run: analyze and report but don't write anything.
    DryRun,
}

impl Default for ImprovementConfig {
    fn default() -> Self {
        Self {
            min_confidence: Confidence::High,
            min_evidence_count: 3,
            output_mode: OutputMode::ProposalOnly,
            cooldown_hours: 24,
            max_improvements_per_run: 3,
        }
    }
}
```

### 4.2 ImprovementDetector

Filters raw `AnalysisFinding`s into actionable improvements.

```rust
// fx-improve/src/detector.rs

/// An improvement candidate that passed all filtering gates.
#[derive(Debug, Clone)]
pub struct ImprovementCandidate {
    pub finding: AnalysisFinding,
    /// Unique fingerprint for deduplication (hash of pattern_name + description).
    pub fingerprint: String,
}

pub struct ImprovementDetector {
    config: ImprovementConfig,
    /// Previously acted-on fingerprints (loaded from ~/.fawx/improvements/history.jsonl).
    known_fingerprints: HashSet<String>,
}

impl ImprovementDetector {
    pub fn new(config: ImprovementConfig, data_dir: &Path) -> Result<Self, ImprovementError>;

    /// Filter findings to actionable improvement candidates.
    ///
    /// Filters applied (in order):
    /// 1. Confidence >= config.min_confidence
    /// 2. Evidence count >= config.min_evidence_count
    /// 3. suggested_action is Some (finding must have a suggested fix)
    /// 4. Fingerprint not in known_fingerprints (no re-proposals)
    /// 5. Truncate to config.max_improvements_per_run
    pub fn detect(&self, findings: &[AnalysisFinding]) -> Vec<ImprovementCandidate>;

    /// Record that an improvement was acted on (persisted to history).
    pub fn record_acted(&mut self, fingerprint: &str) -> Result<(), ImprovementError>;
}
```

### 4.3 ImprovementPlanner

Takes filtered candidates and generates concrete fix plans using an LLM.

```rust
// fx-improve/src/planner.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixPlan {
    pub candidate: ImprovementCandidate,
    /// Which file(s) to modify.
    pub target_files: Vec<PathBuf>,
    /// Natural language description of the fix.
    pub fix_description: String,
    /// Concrete code changes (if determinable).
    /// None if the fix requires human judgment.
    pub code_changes: Option<Vec<FileChange>>,
    /// Risk assessment.
    pub risk: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: PathBuf,
    pub description: String,
    /// The proposed new content for the file.
    /// None if only a description is provided (human must implement).
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RiskLevel {
    /// Safe: test-only, config, or documentation change.
    Low,
    /// Moderate: behavioral change with test coverage.
    Medium,
    /// High: architectural change, kernel modification, or untestable.
    High,
}

pub struct ImprovementPlanner;

impl ImprovementPlanner {
    /// Generate fix plans for improvement candidates.
    ///
    /// Uses the LLM to:
    /// 1. Identify target files from the codebase context
    /// 2. Propose concrete changes
    /// 3. Assess risk level
    ///
    /// Candidates that can't produce a deterministic plan get
    /// code_changes: None (proposal-only, human implements).
    pub async fn plan(
        candidates: &[ImprovementCandidate],
        provider: &dyn CompletionProvider,
        repo_root: &Path,
    ) -> Result<Vec<FixPlan>, ImprovementError>;
}
```

### 4.4 ImprovementExecutor

Turns fix plans into proposals, branches, or dry-run reports.

```rust
// fx-improve/src/executor.rs

#[derive(Debug)]
pub struct ExecutionResult {
    pub proposals_written: Vec<PathBuf>,
    pub branches_created: Vec<String>,
    pub skipped: Vec<(String, String)>,  // (fingerprint, reason)
}

pub struct ImprovementExecutor {
    config: ImprovementConfig,
    proposal_writer: ProposalWriter,
    repo_root: PathBuf,
}

impl ImprovementExecutor {
    pub fn new(
        config: ImprovementConfig,
        proposals_dir: PathBuf,
        repo_root: PathBuf,
    ) -> Self;

    /// Execute fix plans according to the configured output mode.
    ///
    /// - DryRun: return plans without writing anything
    /// - ProposalOnly: write proposals to ~/.fawx/proposals/
    /// - ProposalWithBranch: write proposals AND create git branches with changes
    ///
    /// For ProposalWithBranch:
    /// 1. Create branch: improve/{fingerprint_short}
    /// 2. Apply code_changes (if present)
    /// 3. git_checkpoint with descriptive message
    /// 4. Write proposal referencing the branch
    pub fn execute(
        &self,
        plans: &[FixPlan],
        detector: &mut ImprovementDetector,
    ) -> Result<ExecutionResult, ImprovementError>;
}
```

### 4.5 Top-Level Pipeline

```rust
// fx-improve/src/lib.rs

pub async fn run_improvement_cycle(
    signal_store: &SignalStore,
    llm_provider: &dyn CompletionProvider,
    config: &ImprovementConfig,
    data_dir: &Path,
    repo_root: &Path,
    proposals_dir: &Path,
) -> Result<ExecutionResult, ImprovementError> {
    // 1. Run analysis engine
    let engine = AnalysisEngine::new(signal_store);
    let findings = engine.analyze(llm_provider).await?;

    // 2. Filter to actionable candidates
    let mut detector = ImprovementDetector::new(config.clone(), data_dir)?;
    let candidates = detector.detect(&findings);

    if candidates.is_empty() {
        return Ok(ExecutionResult::empty());
    }

    // 3. Generate fix plans
    let plans = ImprovementPlanner::plan(&candidates, llm_provider, repo_root).await?;

    // 4. Execute plans (proposals/branches/dry-run)
    let executor = ImprovementExecutor::new(
        config.clone(),
        proposals_dir.to_path_buf(),
        repo_root.to_path_buf(),
    );
    executor.execute(&plans, &mut detector)
}
```

### 4.6 TUI Integration

```rust
// In fx-cli/src/tui.rs

// New /improve command
// Triggered by: /improve [--dry-run] [--mode proposal|branch]
// Also called automatically post-session when enabled

async fn handle_improve_command(&mut self, args: &str) -> Result<(), Box<dyn Error>> {
    let config = self.build_improvement_config(args);
    let result = run_improvement_cycle(
        &self.signal_store,
        &self.llm_provider,
        &config,
        &self.data_dir,
        &self.repo_root,
        &self.proposals_dir,
    ).await?;
    self.display_improvement_result(&result);
    Ok(())
}
```

---

## 5. Implementation Plan

### Phase 1: Core Pipeline (fx-improve crate)

1. Create `fx-improve` crate with Cargo.toml (deps: fx-analysis, fx-propose, fx-memory, fx-llm, fx-core, serde, thiserror)
2. Implement `ImprovementConfig` with `Default` and validation
3. Implement `ImprovementDetector` with fingerprinting, filtering, history persistence
4. Implement `ImprovementPlanner` with LLM-powered plan generation
5. Implement `ImprovementExecutor` with proposal writing + optional branch creation
6. Implement `run_improvement_cycle` top-level pipeline
7. Unit tests for each component

### Phase 2: TUI Wiring

1. Add `/improve` command to `parse_command` in tui.rs
2. Wire `run_improvement_cycle` with existing TUI state (signal store, LLM provider, paths)
3. Display results: proposals written, branches created, dry-run findings
4. Add optional post-session trigger (config flag: `self_improve.auto_trigger = false`)

### Phase 3: Post-Session Automation (opt-in)

1. Add `self_improve` section to `~/.fawx/config.toml`
2. After session ends, check cooldown, then run improvement cycle if enabled
3. Results written to proposals dir; user reviews next session

---

## 6. Test Plan

### 6.1 ImprovementDetector tests

| Test | Assertion |
|------|-----------|
| `filters_below_confidence_threshold` | Low-confidence findings excluded when min is High |
| `filters_insufficient_evidence` | Findings with < min_evidence_count excluded |
| `filters_without_suggested_action` | Findings with no suggested_action excluded |
| `filters_known_fingerprints` | Previously acted-on patterns not re-proposed |
| `respects_max_improvements_per_run` | Output truncated to limit |
| `fingerprint_is_deterministic` | Same pattern_name + description → same fingerprint |
| `fingerprint_differs_for_different_findings` | Different patterns → different fingerprints |
| `record_acted_persists_to_disk` | History JSONL updated after recording |
| `detect_with_empty_findings_returns_empty` | No panic on empty input |
| `detect_with_all_filtered_returns_empty` | All below threshold → empty result |

### 6.2 ImprovementPlanner tests

| Test | Assertion |
|------|-----------|
| `generates_plan_for_deterministic_fix` | Code changes populated for clear fix |
| `generates_proposal_only_for_ambiguous_fix` | code_changes is None for judgment-required |
| `risk_assessment_maps_correctly` | Test-only = Low, behavioral = Medium, kernel = High |
| `plan_with_no_candidates_returns_empty` | Empty input → empty output |
| `plan_handles_llm_error_gracefully` | Returns ImprovementError, not panic |

### 6.3 ImprovementExecutor tests

| Test | Assertion |
|------|-----------|
| `dry_run_writes_nothing` | No proposals or branches created |
| `proposal_only_writes_proposal_no_branch` | Proposal file created, no git operations |
| `proposal_with_branch_creates_both` | Proposal + branch with checkpoint |
| `records_fingerprints_after_execution` | Detector history updated |
| `skips_plans_without_code_changes_in_branch_mode` | Graceful degradation to proposal-only |

### 6.4 Pipeline integration tests

| Test | Assertion |
|------|-----------|
| `full_cycle_empty_signals_produces_no_improvements` | Clean run, no output |
| `full_cycle_with_recurring_friction_produces_proposal` | End-to-end: signals → finding → proposal file |
| `cooldown_prevents_immediate_rerun` | Second run within cooldown_hours returns empty |
| `config_validation_rejects_zero_evidence_threshold` | Bad config caught early |

---

## 7. Data Flow Diagram

```
~/.fawx/signals/*.jsonl
        │
        ▼
  AnalysisEngine.analyze()          ← LLM call (pattern detection)
        │
        ▼
  Vec<AnalysisFinding>
        │
        ▼
  ImprovementDetector.detect()      ← Filters: confidence, evidence count,
        │                              suggested_action, known fingerprints
        ▼
  Vec<ImprovementCandidate>
        │
        ▼
  ImprovementPlanner.plan()         ← LLM call (fix planning)
        │
        ▼
  Vec<FixPlan>
        │
        ▼
  ImprovementExecutor.execute()     ← Mode: DryRun / ProposalOnly / ProposalWithBranch
        │
        ├──► ~/.fawx/proposals/*.md           (always, except DryRun)
        ├──► git branch: improve/{fingerprint} (ProposalWithBranch only)
        └──► ~/.fawx/improvements/history.jsonl (fingerprint dedup record)
```

---

## 8. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| LLM hallucinating non-existent patterns | Evidence count threshold (≥3 signals); human reviews all proposals |
| Fix plan modifies wrong files | ProposalWithBranch creates a reviewable branch; human merges |
| Improvement loop runs too aggressively | Cooldown (24h default), max per run (3), manual trigger by default |
| Proposal spam for minor issues | High confidence default; min 3 evidence signals |
| Circular fixes (fix creates new friction) | Fingerprint history prevents re-proposing same pattern; human gate |
| LLM cost for analysis + planning | Two LLM calls per cycle (analysis + planning); bounded by max_improvements_per_run |

---

## 9. Estimated Complexity

| Phase | Scope | Effort | Risk |
|-------|-------|--------|------|
| Phase 1: Core pipeline | ~400 lines code, ~300 lines tests | 1-2 days | Low — builds on existing crates |
| Phase 2: TUI wiring | ~100 lines code, ~50 lines tests | 0.5 day | Low — pattern exists for other commands |
| Phase 3: Post-session auto | ~80 lines code, ~40 lines tests | 0.5 day | Low — config flag + trigger point |

**Total:** ~580 lines code + ~390 lines tests. 2-3 focused implementation days.
