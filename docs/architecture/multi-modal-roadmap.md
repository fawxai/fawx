# Multi-Modal Roadmap: From LLM Agent to Self-Extending Agentic Engine

**Date:** 2026-03-01
**Status:** Vision document — captures architectural direction and phased delivery plan.

---

## Thesis

Fawx is not an LLM wrapper. It is a **self-improving execution kernel** that currently uses LLMs as its reasoning engine. The kernel's trait-based architecture (Skill, ToolExecutor, MemoryProvider) is modality-agnostic — the same loop, signal, and memory infrastructure that powers text reasoning can power vision, audio, and robotics.

Multi-modal capabilities are **additive, not architectural** — through Phase 6, you don't redesign Fawx, you load more skills. Phase 7+ (SuperFawx multi-kernel orchestration) requires kernel evolution at the code level, but the signal, memory, and self-improvement infrastructure established in Phases 1–5 remains unchanged.

---

## Current Architecture

The kernel runs a **7-step agentic loop**. The diagram below shows all seven steps — five core reasoning steps (Perceive through Verify) plus two cross-cutting infrastructure steps (Signal Collection and Memory) that run alongside:

```
User Input (text)
  → 1. Perceive → 2. Reason (LLM) → 3. Decide → 4. Act (tools) → 5. Verify
         ↓                                                            ↓
     6. Signal Collection ←──────────────────────────────── 6. Signal Collection
         ↓
     7. Memory (persist, analyze, improve)
```

Steps 1–5 form the sequential reasoning pipeline. Steps 6–7 are infrastructure that runs at multiple points in the loop: signals are collected after Perceive (input quality), after Act (tool outcomes), and after Verify (success/failure). Memory is updated at the end of each full cycle.

**What exists today:**
- 7-step agentic loop with native tool calling
- Signal persistence (JSONL, redacted, 30-day retention)
- Structured memory with access tracking
- Plugin system (fx-loadable, fx-skills WASM runtime)
- Three-tier self-modification enforcement (allow/propose/deny)
- Self-building architecture (code generation + git workflow)

---

## Where Signals and Memory Apply in the Model Lifecycle

### Pre-Training (with internet-scale signal data)
- **Curriculum learning**: Signal difficulty gradients order training data
- **Data weighting**: Friction patterns per knowledge domain inform corpus mix
- **Agentic pre-training objectives**: Auxiliary tasks (predict tool, predict success, predict friction) alongside next-token prediction produce natively agentic models
- **Synthetic data generation**: Signal patterns define ideal behavior → generate training examples

### Post-Training (fine-tuning, RLHF, DPO)
- **Self-generated reward signal**: The signal analysis engine identifies matching prompts with divergent outcomes from conversation history — same context, different results — to generate DPO pairs. Friction-heavy outcomes become the rejected sample; success-heavy outcomes become the chosen sample. This requires same-prompt paired comparisons, not simply labeling all friction as "rejected" and all success as "chosen."
- **LoRA adapters**: Small adapters trained on signal-curated conversation data per instance
- **SFT data curation**: Only high-success, low-friction conversations become training examples
- **Safety tuning**: Friction signals on dangerous actions → negative examples

### Inference (real-time, already partially implemented)
- **Memory injection**: Access-count weighted snapshot in system prompt ✅
- **Signal-driven retry**: Friction → verify → retry loop ✅
- **Model routing**: Performance + friction signals per model → automatic routing
- **Procedural memory**: Analysis discovers patterns → writes memories → injected into prompt → behavior changes without touching weights

---

## Perception: Beyond ITT/STT

### MVP: ITT + STT as Skills
For 80% of use cases, wrapping a VLM and Whisper as Fawx Skills is sufficient:
- Image → VLM → text description → LLM reasons
- Audio → Whisper → transcript → LLM reasons
- Signals work immediately (visual confidence, speech clarity)

### Full Perception: Structured Multi-Layer Output

**Vision Pipeline:**
```
Raw frames → Object detection (YOLO/SAM) → scene graph
Raw frames → Depth estimation (MiDaS/ZoeDepth) → depth map
Raw frames → Segmentation → actionable regions
Frame sequence → Optical flow → motion vectors
High-level → VLM for semantic reasoning

Output: PerceptionResult {
    scene_text: String,                          // for LLM reasoning
    objects: Vec<DetectedObject>,                // for robot planning
    depth_map: Option<DepthMap>,                 // for collision avoidance
    alerts: Vec<PerceptionAlert>,                // for signal generation
}
```

**Audio Pipeline:**
```
Raw audio → STT → transcript
Raw audio → Speaker diarization → who said what
Raw audio → Emotion analysis → urgency/sentiment
Raw audio → Sound classification → environmental events
Raw audio → Spatial processing → source direction

Output: AudioPerception {
    transcript: String,
    speaker: Option<SpeakerId>,
    emotion: EmotionEstimate,
    direction: Option<Angle>,
    environmental: Vec<SoundEvent>,
}
```

**Compute Considerations:**
The full perception pipeline (YOLO + depth + segmentation + optical flow + VLM) is expensive — running all stages on every frame is neither practical nor necessary. Pipeline stages are **task-driven**: the orchestrator activates only the stages relevant to the current task. For example:
- A "describe the scene" request activates VLM only — no depth estimation or optical flow.
- A "navigate to the door" robotics command activates object detection + depth + segmentation, skips emotion analysis.
- Optical flow activates only when motion tracking is needed (e.g., following a moving object).

A compute budget per perception cycle enforces this: each task specifies a latency ceiling (e.g., 100ms for real-time robotics, 2s for ambient scene analysis), and the pipeline scheduler selects stages that fit within the budget.

**Why both layers matter:**
- LLM reasons over the **semantic** layer (text descriptions, transcripts)
- Robotics controller consumes the **numerical** layer (coordinates, depth, force vectors)
- Both layers produce signals into the same analysis pipeline

### Perception and Memory

Perception results are ephemeral by default — they flow through the reasoning pipeline and produce signals. However, **perception summaries** are written to memory for cross-session recall. For example:
- A scene description from a VLM ("user's desk has a red mug, two monitors, and a plant") persists as a memory entry with a vision source tag (whether via an extended `MemorySource` enum or the free-form `tags` field — see Design Principle #3).
- Audio identification events ("user typically speaks with background music playing") persist similarly with an audio source tag.

This allows the kernel to reference past perceptions without re-processing raw sensor data. Memory access tracking and signal-driven relevance filtering apply to perception memories the same way they apply to text-based memories.

### The Collapsing Boundary
Modern multi-modal models (GPT-4o, Gemini) process visual/audio tokens directly alongside text. The "conversion to text" happens inside the model. For pure agent tasks, ITT/STT is becoming an internal model capability, not a pipeline stage. For robotics, structured numerical output remains essential alongside semantic understanding.

**Fawx's strategy:** Support both the discrete pipeline path (ITT/STT as explicit skills) and native multi-modal inference (passing raw tokens directly to a capable model). The model's capabilities determine which path is used at runtime — if the active model handles vision natively, skip the ITT skill; if it's text-only, route through the VLM skill. This dual-path approach avoids betting on a single architecture and ensures Fawx works with both current text-centric models and emerging multi-modal ones.

---

## SuperFawx: Multi-Kernel Architecture

### Single Kernel (current)
One loop, one brain, one signal stream.

### Multi-Kernel (target)
```
SuperFawx Orchestrator (Fawx kernel loop)
├── LLM Kernel      (text reasoning, planning, code gen)
├── Diffusion Kernel (image gen, scene understanding, visual prediction)
├── Audio Kernel     (speech, music, sound analysis)
└── Robotics Kernel  (motion planning, sensor fusion, actuator control)
```

> **Kernel immutability note:** ENGINEERING.md §6 establishes that the kernel is immutable at runtime. SuperFawx does not violate this invariant — multi-kernel orchestration is a **compile-time** architectural extension, not a runtime modification. The orchestrator and sub-kernel dispatch logic are compiled into the binary; they do not mutate the kernel's execution path at runtime. The self-modification enforcement system (allow/propose/deny) continues to govern all runtime behavior changes.

**Orchestrator responsibilities:**
- Perceive: multi-modal input routing
- Reason: LLM-based planning + task decomposition
- Decide: which sub-kernel handles which subtask
- Act: dispatch to sub-kernels
- Verify: cross-modal verification
- Signal aggregation from all sub-kernels

**Sub-kernel properties:**
- Own loop, own signal collector, own memory scope
- Modality-specific loop parameters (robotics: fast cycles; diffusion: batch cycles)
- Signal bubbling: sub-kernel signals propagate to orchestrator
- Resource isolation: GPU contention managed at orchestrator level

### Cross-Kernel Communication

Sub-kernels communicate exclusively through the orchestrator in a **hub-and-spoke** topology — no direct peer-to-peer messaging between sub-kernels. This keeps the orchestrator as the single point of coordination and prevents emergent coupling between modalities.

Communication uses structured message types:
- **TaskDispatch**: Orchestrator → sub-kernel. Contains task description, input data references, deadline, and priority.
- **TaskResult**: Sub-kernel → orchestrator. Contains output data, signals collected, and completion status.
- **DataRequest**: Sub-kernel → orchestrator → sub-kernel. When one kernel needs another's output (e.g., robotics needs depth map from vision), the request routes through the orchestrator, which decides whether to fulfill it from cache or dispatch a new perception task.
- **SignalReport**: Sub-kernel → orchestrator. Periodic signal bubble-up outside of task completion.

The orchestrator maintains a shared data cache of recent sub-kernel outputs, so repeated cross-modal queries don't require redundant computation.

### Sub-Kernel Failure Modes

Sub-kernels are isolated processes. A failure in one must not cascade to the others or crash the orchestrator.

**Failure isolation:**
- Each sub-kernel runs in its own process/sandbox. A panic, OOM, or hang in the Diffusion Kernel does not affect the LLM Kernel or Robotics Kernel.
- The orchestrator monitors sub-kernel health via heartbeats (configurable interval, default: 1s).

**Timeout and recovery:**
- Every `TaskDispatch` includes a deadline. If a sub-kernel does not return a `TaskResult` before the deadline, the orchestrator marks the task as timed out.
- On timeout: the orchestrator cancels the stuck task, logs a friction signal, and attempts recovery — restart the sub-kernel if it's unresponsive, or re-route the task to a fallback strategy (e.g., skip depth estimation and proceed with VLM-only scene understanding).

**Graceful degradation:**
- The orchestrator maintains a capability map of which sub-kernels are healthy. If a sub-kernel is down, the orchestrator degrades gracefully: tasks requiring that modality are either deferred (with a signal noting reduced capability) or handled by a simpler fallback (e.g., text-only reasoning when vision is unavailable).
- Repeated failures trigger a friction signal pattern that surfaces in signal analysis, enabling the self-extension loop to propose fixes or alternative implementations.

---

## Robotics: Two-Tier Control

### The Problem
LLM inference: ~3 seconds per decision. Robot actuators: ~1 millisecond control loops. A robot cannot wait for an LLM to decide whether to adjust grip force.

### The Solution: Hierarchical Control
```
Fawx Kernel (slow loop, seconds)
  └── Plans: "pick up cup, move to table"
      └── Dispatches to:
          Robotics Controller (fast loop, milliseconds)
            ├── PID/MPC: real-time motor control
            ├── Safety monitor: force/velocity limits
            └── Signals → bubble up to kernel
```

**Fawx handles:** strategic planning, task decomposition, world modeling, error recovery
**Controller handles:** trajectory execution, reflex responses, safety enforcement

### Safety Enforcement
Three-tier path system applied to actuator commands:
- **Allow**: Safe movements within calibrated workspace
- **Propose**: Movements near humans, new environments → human confirms
- **Deny**: Force limit exceedance, geofence violations, e-stop override

### Simulation-as-Verify
Before real-world execution, the Verify step runs the planned action in a physics simulator. Friction signals from simulation are cheaper than friction signals from real-world damage.

---

## Self-Extension: The Endgame

The most revolutionary capability: Fawx discovers it needs new capabilities and bootstraps them.

### The Self-Extension Loop
1. User asks for unsupported capability → tool call fails → **friction signal**
2. Pattern accumulates → signal analysis discovers "capability gap"
3. Fawx proposes a new Skill implementation wrapping the needed model/API
4. **Propose-tier enforcement** → human reviews generated code
5. Approved skill loaded via plugin registry
6. Next request succeeds → **success signals** confirm capability works
7. New modality's signals feed back into the improvement loop

### Mapped to Existing Subsystems
| Step | Subsystem | Status |
|------|-----------|--------|
| Friction on missing capability | Signal persistence | ✅ Merged |
| Pattern discovery | Signal analysis engine | Planned |
| Procedural memory write | Memory system | ✅ Merged |
| Code generation for new skill | LLM + self-building | Planned (#1001) |
| Human approval gate | Path enforcement | ✅ Merged |
| Plugin loading | fx-loadable + fx-skills | ✅ Merged |
| Feedback loop | Signal collection | ✅ Built into kernel |

### Safeguards
- **Proposal cooldown**: Prevent friction → propose → bad skill → more friction loops. Default: **24-hour cooldown** between proposals for the same capability gap (configurable via `self_extension.cooldown_hours`).
- **Attempt limits**: Max **3 proposals** per capability gap before escalating to human for manual intervention (configurable via `self_extension.max_proposals_per_gap`). After the limit, the gap is flagged for human design input rather than continued auto-generation.
- **Human-in-the-loop**: Propose-tier is non-negotiable for auto-generated skills.
- **Sandboxing**: Generated skills run in WASM sandbox with limited permissions.

---

## Phased Delivery

> **Phases 1–4** (completed) built the foundation described in "Current Architecture" above: the 7-step agentic loop engine (Phase 1), signal persistence and structured memory (Phase 2), plugin system and WASM skill runtime (Phase 3), and three-tier self-modification enforcement (Phase 4).

### Phase 5: Signal Intelligence (current plan)
- Signal analysis engine — batch LLM analysis, pattern discovery
- Smart injection — relevance-filtered memory in system prompt
- Loop closure — patterns → procedural memory → behavior change

**Preconditions:** Signal persistence and structured memory merged and stable (both ✅).
**Done when:** Signal analysis engine runs batch analysis on collected signals, identifies at least friction patterns and capability gaps, and writes procedural memories that measurably change kernel behavior in subsequent conversations. **Quality indicator:** overall friction rate on previously-problematic task categories drops measurably after procedural memory injection (target: ≥15% reduction).

### Phase 6: Multi-Modal Foundation
- #1015 — Perception types in fx-core
- #1016 — VLM Skill (vision-language model)
- #1017 — STT/TTS Skill (speech and audio)

> **⚠️ fx-core governance note (#1015):** Adding perception types (`PerceptionResult`, `AudioPerception`, `DetectedObject`, etc.) to fx-core is a **core-layer change**. fx-core is the foundation crate that all other crates depend on — changes here cascade everywhere. This requires careful design review: types should be minimal, stable, and forward-compatible. Propose a RFC-style design doc for the perception type hierarchy before implementation. Prefer trait-based abstractions that new modalities can implement over concrete structs that require fx-core changes for each new sensor type.

**Preconditions:** Phase 5 complete — signal analysis must be operational so that perception signals flow through the existing pipeline without special-casing.
**Done when:** VLM and STT skills load via the plugin system, produce `PerceptionResult`/`AudioPerception` outputs, and their signals are collected and analyzed by the Phase 5 infrastructure. **Quality indicator:** friction rate on image-related and audio-related tasks drops below the pre-Phase-6 baseline (measured via signal analysis).

### Phase 7: Cross-Modal Intelligence
- #1018 — Cross-modal signal aggregation
- #1019 — Sub-kernel orchestration (SuperFawx)

**Preconditions:** Phase 6 complete — at least two modalities (vision + audio) operational and producing signals.
**Done when:** SuperFawx orchestrator manages multiple sub-kernels, routes tasks by modality, aggregates cross-modal signals, and handles sub-kernel failures gracefully. Cross-modal signal patterns (e.g., "user sounds frustrated AND visual context shows error screen") are detected by the analysis engine. **Quality indicator:** cross-modal task completion rate exceeds single-modal baseline, and cross-modal friction patterns are surfaced within one analysis cycle.

### Phase 8: Embodied Agent
- #1020 — Robotics control interface
- #1021 — Self-extension discovery

**Preconditions:** Phase 7 complete — multi-kernel orchestration stable. For robotics: simulation environment available for Verify step.
**Done when:** Two-tier control (kernel + robotics controller) executes planned actions in simulation with safety enforcement. Self-extension loop demonstrates end-to-end: friction signal → gap discovery → skill proposal → human approval → loaded skill → success signal. **Quality indicator:** simulation-verified action plans have a ≥90% real-world execution success rate; self-extended skills resolve their originating friction pattern within 3 uses.

### Phase 9: Model Training Loop
- DPO pair generation from signal history
- LoRA adapter training from curated conversations
- Agentic pre-training objective design (if scale justifies)

**Preconditions:** Phases 5–8 complete — sufficient signal history accumulated across modalities to produce meaningful training data.
**Done when:** DPO pairs generated from signal history produce measurable preference alignment improvement. LoRA adapters trained on curated data show measurable performance gains on Fawx-specific tasks compared to base model. **Quality indicator:** fine-tuned model achieves ≥10% win rate improvement over base model on a held-out set of Fawx task evaluations, measured by signal-derived success/friction ratios.

> **⚠️ Privacy and data governance:** Training on signal and conversation data has privacy implications. By default, all signal data is **local-only** — no data leaves the device without explicit user consent. DPO pair generation, LoRA training data curation, and any model fine-tuning operate exclusively on local data. If cloud compute is used for training (see below), only explicitly opted-in, anonymized data is uploaded, and the user controls what is included. This is non-negotiable: the user owns their data.
>
> **⚠️ Compute feasibility note:** LoRA training and DPO fine-tuning require GPU compute (typically 1+ NVIDIA GPU with 16GB+ VRAM) that may not be available on local development hardware. Phase 9 may require cloud compute (e.g., RunPod, Lambda Labs, or equivalent GPU rental) or defer until local GPU hardware is sufficient. The signal collection and DPO pair generation steps (data preparation) can run locally; only the training step requires GPU. Plan accordingly — this phase's timeline depends on compute access, not just code readiness.

---

## Design Principles

1. **Additive, not architectural (through Phase 6).** Each modality is a new skill, not a new system. Phase 7+ extends the kernel's orchestration capabilities at the code level (compile-time), but the signal/memory/improvement infrastructure remains unchanged.
2. **Signals are the universal language.** Every modality produces signals into the same pipeline.
3. **Memory is modality-tagged.** New modalities need a way to tag their memory entries by origin. The [memory-and-signals-plan](memory-and-signals-plan.md) deliberately keeps the `MemorySource` enum minimal (`User`, `SignalAnalysis`, `Consolidation`) and uses free-form `tags: Vec<String>` for emergent categorization. The `MemorySource::Vision`, `MemorySource::Robotics` notation used elsewhere in this roadmap is aspirational — the actual implementation may extend the enum with new variants (possibly including an `Other(String)` fallback for unforeseen modalities), or use the existing tags field with conventional values like `"vision"`, `"robotics"`. The right approach will be decided during Phase 6 design review (#1015).
4. **Safety scales with capability.** Three-tier enforcement applies to file writes and robot arms equally.
5. **Self-improvement is universal.** The signal → analysis → memory → behavior loop works for any decision-making system.
6. **The model is swappable.** The kernel doesn't care what powers the Reason step. LLMs today, specialized models tomorrow.

---

## Glossary

| Term | Definition |
|------|-----------|
| **SuperFawx** | The multi-kernel orchestration architecture where a primary Fawx kernel manages multiple specialized sub-kernels (vision, audio, robotics, etc.). |
| **Sub-kernel** | A specialized Fawx kernel instance within SuperFawx, responsible for a single modality (e.g., LLM Kernel, Diffusion Kernel, Audio Kernel, Robotics Kernel). Each has its own loop, signal collector, and memory scope. |
| **Friction signal** | A signal emitted when something goes wrong or is suboptimal during kernel execution — tool failures, user corrections, timeouts, low-confidence outputs. The raw material for self-improvement. |
| **Signal analysis engine** | A batch process that analyzes accumulated signals to discover patterns — recurring friction, capability gaps, performance trends. Drives procedural memory writes and self-extension proposals. |
| **Propose-tier** | The middle tier of Fawx's three-tier self-modification enforcement (Allow / Propose / Deny). Actions at propose-tier require human approval before execution. Used for auto-generated skills, movements near humans, and any action with meaningful risk. |
| **fx-core** | The foundation Rust crate in the Fawx engine. All other crates depend on it. Changes to fx-core cascade everywhere, so modifications require careful design review. |
| **MemorySource** | A typed Rust enum identifying the origin of a memory entry (e.g., `User`, `SignalAnalysis`, `Consolidation`). Multi-modal work would extend this with variants like `Vision`, `Robotics`, `Audio`. |
| **DPO (Direct Preference Optimization)** | A post-training technique that aligns model behavior using paired examples: a "chosen" (preferred) and "rejected" (dispreferred) response to the same prompt. Fawx generates these pairs from signal history. |
| **LoRA (Low-Rank Adaptation)** | A parameter-efficient fine-tuning method that trains small adapter weights instead of modifying the full model. Enables per-instance specialization without full retraining. |
| **Procedural memory** | Memory entries that encode learned behaviors and patterns, written by the signal analysis engine. Injected into the system prompt to change kernel behavior without modifying model weights. |
| **Three-tier enforcement** | Fawx's safety system: **Allow** (safe, execute immediately), **Propose** (risky, require human approval), **Deny** (forbidden, always blocked). Applied uniformly across all capabilities. |
| **Self-extension** | The capability for Fawx to discover it lacks a needed skill (via friction signals), generate a new skill implementation, submit it for human review, and load it into the plugin system. |

---

*This document captures the architectural vision as of 2026-03-01. Implementation follows the phased plan above, with each phase building on the previous. The kernel, signal, and memory infrastructure already in place is the foundation for everything that follows.*
