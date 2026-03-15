# Proof of Fitness — Adversarial Consensus Self-Improvement Protocol

## Overview

A distributed protocol where fleet nodes **compete** to produce the best self-improvement for a detected signal, and the network reaches **consensus** on which candidate wins. The winning change is committed to a tamper-evident chain. This replaces the current single-agent `/improve` flow with a multi-agent adversarial evolution system.

## Core Insight

Karpathy's auto-research: one GPU, one agent, iterating serially on experiments.
Proof of Fitness: N GPUs, N agents, competing in parallel — natural selection for software.

The fitness function is **self-referential**: the same signal analysis that identifies problems also evaluates solutions. Better analysis → better fitness evaluation → better self-improvement → which improves analysis. Recursive improvement with a hard safety floor (the kernel).

## Architecture

### New Crate: `engine/crates/fx-consensus`

Core engine crate (not a WASM skill — needs fleet coordination, chain storage, and deep integration).

### Data Types

```
Experiment
├── id: uuid
├── trigger: Signal (from /analyze)
├── hypothesis: String ("parallelizing tool calls reduces token waste")
├── fitness_criteria: Vec<FitnessCriterion>
│   ├── name: String ("token_reduction")
│   ├── metric: MetricType (Lower/Higher/Boolean)
│   └── weight: f64
├── scope: ModificationScope
│   ├── allowed_files: Vec<PathPattern>
│   └── tier: ProposalTier (Tier1/Tier2 only, never Tier3)
├── timeout: Duration
├── min_candidates: u32
└── created_at: Timestamp

Candidate
├── id: uuid
├── experiment_id: uuid
├── node_id: NodeId
├── patch: UnifiedDiff (the actual code change)
├── approach: String (plain-text explanation of strategy)
├── self_metrics: BTreeMap<String, f64> (proposer's own measurements)
└── created_at: Timestamp

Evaluation
├── candidate_id: uuid
├── evaluator_id: NodeId
├── fitness_scores: BTreeMap<String, f64> (per-criterion scores)
├── safety_pass: bool
├── signal_resolved: bool (does the triggering signal disappear?)
├── regression_detected: bool (do OTHER signals appear?)
├── notes: String
└── created_at: Timestamp

ConsensusResult
├── experiment_id: uuid
├── winner: Option<CandidateId>
├── candidates: Vec<CandidateId>
├── evaluations: Vec<Evaluation>
├── aggregate_scores: BTreeMap<CandidateId, f64>
├── decision: Decision (Accept/Reject/Inconclusive)
└── timestamp: Timestamp
```

### Chain

Append-only tamper-evident log. Each entry references the previous entry's hash.

```
ChainEntry
├── index: u64
├── previous_hash: SHA-256
├── experiment: Experiment
├── result: ConsensusResult
├── winning_patch: Option<UnifiedDiff>
├── applied_at: Option<Timestamp>
└── hash: SHA-256 (of all above fields)

Chain
├── entries: Vec<ChainEntry>
├── head_hash: SHA-256
└── storage: JsonFileChainStorage (MVP) / ReplicatedStorage (future)
```

Integrity: any node can verify the full chain by recomputing hashes. Tampering breaks the hash chain.

### Protocol Flow

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐
│  /analyze   │────▶│  Experiment   │────▶│  Broadcast    │
│  detects    │     │  created      │     │  to fleet     │
│  signal     │     │              │     │              │
└─────────────┘     └──────────────┘     └──────┬───────┘
                                                │
                    ┌───────────────────────────┬┘
                    ▼                           ▼
              ┌──────────┐               ┌──────────┐
              │  Node A   │               │  Node B   │
              │  generates│               │  generates│
              │  candidate│               │  candidate│
              └─────┬────┘               └─────┬────┘
                    │                           │
                    └───────────┬───────────────┘
                                ▼
                    ┌──────────────────┐
                    │  Candidates      │
                    │  collected       │
                    └────────┬─────────┘
                             │
                    ┌────────┴─────────┐
                    ▼                  ▼
              ┌──────────┐      ┌──────────┐
              │  Node A   │      │  Node B   │
              │  evaluates│      │  evaluates│
              │  ALL      │      │  ALL      │
              │  candidates│     │  candidates│
              └─────┬────┘      └─────┬────┘
                    │                  │
                    └────────┬─────────┘
                             ▼
                    ┌──────────────────┐
                    │  Consensus       │
                    │  computed        │
                    │  (weighted vote) │
                    └────────┬─────────┘
                             │
                    ┌────────┴─────────┐
                    ▼                  ▼
              ┌──────────┐      ┌──────────┐
              │  Winner   │      │  Chain    │
              │  applied  │      │  entry    │
              │  to code  │      │  recorded │
              └──────────┘      └──────────┘
```

### Consensus Algorithm (MVP)

Simple weighted-average fitness scoring:

1. For each candidate, collect all evaluations from other nodes (self-evaluation excluded — adversarial)
2. For each criterion, compute weighted average across evaluators
3. Multiply by criterion weight, sum for total fitness score
4. Candidate with highest total score wins
5. Winner must pass ALL safety checks from ALL evaluators (unanimous safety)
6. Winner must have `signal_resolved = true` from majority of evaluators
7. If no candidate passes safety + signal resolution: experiment result = Reject

### Fitness Evaluation

Each evaluating node:
1. Applies the candidate's patch to a clean working copy
2. Builds and runs tests (must pass — binary gate)
3. Runs the triggering signal's analysis again — does the signal disappear?
4. Runs full signal analysis — do new signals appear? (regression check)
5. Measures fitness criteria (token count on benchmark task, completion rate, etc.)
6. Produces structured Evaluation

### Safety Invariants (Non-Negotiable)

1. **Kernel is never modified** — Tier 3 paths are excluded from all experiments
2. **ProposalGateExecutor validates scope** — candidates that touch forbidden paths are rejected before evaluation
3. **Unanimous safety** — one evaluator flagging safety = candidate disqualified
4. **Chain is append-only** — no rewriting history
5. **Human override** — owner can veto any consensus result before application
6. **Rollback** — fawx-ripcord can revert any applied change

## Implementation Phases

### Phase 1: Core Types + Chain (fx-consensus crate)
- All data types above
- Chain with JSON file storage
- SHA-256 hash computation + integrity verification
- Chain append, read, verify operations
- Tests: chain integrity, hash computation, append-only invariant

### Phase 2: Protocol Engine
- `ConsensusProtocol` trait
- `LocalConsensusEngine` — single-machine implementation (all "nodes" are local subagents)
- Experiment lifecycle: create → collect candidates → evaluate → consensus → apply
- Fitness evaluation harness (apply patch, build, test, measure)
- Tests: full protocol flow with mock nodes

### Phase 3: Fleet Integration
- Wire `LocalConsensusEngine` into `fx-fleet` NodeTransport
- Broadcast experiments via fleet task router
- Collect candidates/evaluations from remote nodes
- `DistributedConsensusEngine` implementation
- Tests: multi-node protocol with SSH transport

### Phase 4: Signal Integration
- Auto-trigger experiments from `/analyze` signals
- Connect existing `propose_improvement` as candidate generator
- Wire signal analysis as fitness evaluator
- `/experiment create` CLI command
- `/chain` CLI command to view history
- Tests: end-to-end signal → experiment → consensus → application

### Phase 5: TUI Integration
- Active experiment status in TUI
- Chain viewer
- Manual experiment creation UI
- Vote/evaluation progress display

## MVP Scope (Phases 1-2)

The MVP runs on a single machine with subagents acting as "nodes." This proves the protocol works before adding network complexity. A single Fawx instance spawns N subagents, each generates a candidate independently, then N different subagents evaluate all candidates, and consensus is computed.

This is already useful: it turns `/improve` from "one shot, hope it works" into "N competing approaches, best one wins."

## File Structure

```
engine/crates/fx-consensus/
├── Cargo.toml
├── src/
│   ├── lib.rs          — public API
│   ├── types.rs        — Experiment, Candidate, Evaluation, ConsensusResult
│   ├── chain.rs        — Chain, ChainEntry, ChainStorage
│   ├── protocol.rs     — ConsensusProtocol trait + LocalConsensusEngine
│   ├── fitness.rs      — FitnessEvaluator, evaluation harness
│   ├── scoring.rs      — weighted scoring, consensus computation
│   └── error.rs        — ConsensusError
└── tests/
    ├── chain_tests.rs
    ├── protocol_tests.rs
    └── scoring_tests.rs
```

## Dependencies

- `fx-fleet` — node communication (Phase 3+)
- `fx-subagent` — spawning candidate generators and evaluators (Phase 2)
- `fx-analysis` — signal detection and evaluation (Phase 4)
- `fx-loadable` — applying winning changes (Phase 2)
- `sha2` — hash computation
- `serde`, `serde_json` — serialization
- `uuid` — identifiers
- `chrono` — timestamps

## Success Criteria

1. **Phase 1 complete when:** Chain can be created, appended to, verified. Types are correct and serializable. 100% test coverage on chain operations.

2. **Phase 2 complete when:** A single-machine experiment runs end-to-end: signal in → N candidates generated by subagents → cross-evaluated → consensus reached → winning patch applied → chain entry recorded. Demonstrable on a real codebase change.

3. **Demo scenario:** `/analyze` detects "sequential tool calls where parallel is possible." Three subagent "nodes" each propose a different optimization. Cross-evaluation measures token savings. Winner applied. Signal disappears on re-analysis. Chain records the proof.
