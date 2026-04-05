# Fawx Perception Engine — Architecture Thesis

**Date:** 2026-03-25, revised 2026-04-05  
**Authors:** Joe, Clawdio  
**Status:** Vision doc — architecture thesis for a multi-epic program. Not a spec, not a plan, not a roadmap. A direction to take bites out of.

---

## What Changed in This Revision

This revision incorporates a few important corrections:

- **Surprise is a salience signal, not the whole salience function.** Prediction error is useful, but what matters is also conditioned by goal, safety, and uncertainty.
- **Perception for acting systems must be action-conditioned.** The right predictor is not just “what comes next?” but “what comes next given the current state and the action just taken?”
- **The output is a typed belief state, not just detections.** The harness needs uncertainty, persistent tracks, affordances, and probe requests, not only labels and boxes.
- **This is not yet a universal plug-in engine for any harness.** The near-term goal is a reusable observer sidecar for a family of harnesses that implement the observer ABI.
- **This is a local multi-model architecture, not a 100B monolith thesis.** The hot loop optimizes for latency, bandwidth, active parameters, and recurrent state size.

---

## The Core Thesis

Signal is partly surprise. Noise is predictability.

A trained world model processes a sensory stream and its prediction error is a powerful salience signal. If the model predicts the next observation correctly, nothing novel happened. If prediction error spikes, something changed that the model did not expect. That is useful.

But novelty is not the same thing as importance.

A flashing notification may be surprising and irrelevant. A pedestrian steadily approaching a curb may be predictable and critically important. A disabled button that has been disabled for ten seconds may be the only thing that matters for the current task. So prediction error is not “attention for free.” It is one free signal inside a larger attention system.

The perception engine’s job is not “detect every element on screen” or “label every object in a room.” It is:

**Given the current sensory field, recent action, and active goal context, produce the smallest typed belief state that preserves what matters now.**

That belief state includes not just entities and text, but also uncertainty, affordances, and whether the observer needs a better view before the planner should commit.

The compressed latent state remains central. But the latent is not the product. The product is the observer state derived from it.

---

## The Architecture

AI assistants today overuse the LLM. They try to route too much observation, too many decisions, and too many micro-actions through one text model. That is slow, expensive, and architecturally wrong.

The right architecture separates continuous observation, closed-loop control, and slow reasoning, while coupling them explicitly:

- **Observer / Perception** (local, continuous, ~30fps): sensor stream + recent action context → latent world state → typed belief state
- **Planner / Controller** (local, fast, ~10fps): belief state + goal step → actions, retries, recovery, stabilization
- **Reasoning / Orchestrator** (local or remote, slow, on-demand): user intent → decomposition, memory use, judgment calls, exception handling
- **Safety / Permissions** (always in the loop): capability gates, commit boundaries, tripwires, policy checks

The coupling matters:

- The **planner sends recent action context** to perception so the observer can distinguish self-caused change from unexpected change.
- The **observer sends belief state, uncertainty, novelty, and affordances** back to the planner.
- The **observer may request probes** such as hover, scroll, pan, slow down, reobserve, or zoom.
- The **planner and safety layer retain final actuation authority**. Perception can ask for a better observation. It should not own the final motor command in a safety-critical loop.
- The **LLM stays above the event horizon**. It should not schedule detectors frame-by-frame or micromanage tensors.

The LLM is the executive. The planner is the motor cortex. Perception is the observer. Safety is the nervous system that says “not that action, not yet.”

---

## What fawx-eyes Actually Is

fawx-eyes is not one model and not a universal black box. It is a local observer sidecar with four concerns:

1. **Spatial encoder**  
   A pretrained vision backbone turns frames into spatial features.

2. **Temporal world model**  
   A recurrent or state-space model carries forward what remains true, updates on change, and predicts future observations conditioned on recent action.

3. **Perception heads**  
   Domain heads convert latent state into typed outputs: UI entities, OCR, layout, tracks, 3D objects, depth, manipulation targets, and similar.

4. **Observer API**  
   A stable interface exposes belief state to the rest of the harness and accepts targeted requests back from the harness.

The key design move is that the observer is a **service**, not a giant prompt.

---

## The Observer ABI

The observer should be reusable, but only for harnesses that implement the interface. The right abstraction is not “plug this into any harness and it works.” The right abstraction is:

**A typed observer ABI with a stable core envelope and domain-specific profiles.**

### Core envelope

Every observer message should carry the same core fields:

- timestamp / frame id / sensor ids
- current profile and schema version
- prediction error / novelty estimate
- uncertainty and health
- recent action context
- goal relevance, if available
- belief state id / persistent track ids
- optional probe request

### Domain profiles

The payload then varies by profile:

- **ui/v1** — UI elements, text regions, layout, affordances, focused controls
- **world/v1** — objects, depth, free space, egomotion, hazards
- **robot/v1** — objects, grasp candidates, contact state, manipulation affordances, tool pose

### Four observer verbs

Perception networks mesh best with the rest of the system as typed services, not as chatty peer agents. The API should support four kinds of interaction:

- **publish** — stream the current belief state
- **query** — answer focused semantic questions about the current scene
- **watch** — monitor a condition over time and notify when it changes
- **probe_request** — ask the planner for a better observation before committing

This makes perception usable as a tool and, where useful, as a bounded watcher. It avoids turning every detector and tracker into a prose-speaking “subagent.”

---

## The Progression

The runtime pattern can remain stable across stages. The exact degree of backbone transfer is an empirical question.

### Stage 1: Computer Use

- **Input:** screen capture, window metadata, accessibility snapshots, planner action context
- **Observer:** screen encoder → action-conditioned temporal model → UI heads
- **Training supervision:** accessibility tree, OCR targets, event-driven snapshots, action/outcome alignment
- **Planner:** belief state + goal step → keyboard/mouse/scroll actions
- **Value:** Fawx can use software through perception, not by depending on browser automation or native accessibility APIs at runtime
- **Limitation:** screens are an impoverished causal world. A click can teleport to a visually unrelated state. Physics does not constrain UI transitions.

### Stage 2: Wearable Pendant / Assistive Observer

- **Input:** camera, IMU, optional GPS, optional depth, planner/context signals
- **Observer:** same service shape, different heads and likely some backbone retuning
- **Training supervision:** physics, egomotion, weak supervision from human behavior, optional depth/self-supervision
- **Planner:** belief state + context → voice alerts, haptics, navigation prompts, probe behaviors
- **Value:** assistance, scene description, text reading, reminders, navigation support

### Stage 3: Robotic Assistant

- **Input:** cameras, proprioception, force/contact, optional depth/LiDAR, planner action context
- **Observer:** spatial world model with object tracks, affordances, contact-aware state
- **Training supervision:** all of Stage 2 plus motor feedback and manipulation outcomes
- **Planner:** belief state + goal → trajectories, manipulation sequences, recovery behaviors
- **Value:** physical assistance

### What is most likely to transfer across stages

- The **service shape**: observer sidecar + typed ABI + planner/controller + reasoning layer
- The **temporal objectives**: predict what changes, preserve what persists, separate novelty from background
- The **memory pattern**: carry forward latent state rather than re-deriving the entire world from scratch every frame
- Some pretrained visual features and temporal priors, subject to measurement

### What is least likely to transfer cleanly

- The full **perception head**
- The **affordance schema**
- The **action space**
- The exact degree of **backbone reuse** from screens to physical scenes
- The supervision source and label taxonomy

The strong claim is not “one backbone will do all the work.”
The stronger and more defensible claim is that the **runtime pattern, training objectives, and observer ABI** can persist while heads and parts of the backbone change.

---

## Why Physics Is a Richer Supervisor Than the Accessibility Tree

The accessibility tree is a training-time luxury. It exists because screens are arbitrary and underconstrained. A button can be any color, any size, anywhere, and mean anything. Someone has to declare “this is a button.”

The physical world supervises more of itself:

- **Depth** gives geometry
- **Egomotion** gives action-consequence pairing
- **Physics** gives temporal constraints
- **Object permanence** gives consistency
- **Human behavior** gives weak semantic and affordance supervision

That last point needs care. Human behavior is not a literal replacement for all labels. It is a rich weak supervisor. Watching a person approach, grasp, pull, avoid, or inspect things teaches the model a lot about what objects afford. It does not eliminate the need for evaluation or other forms of supervision.

---

## Why Personal Training Data Matters

Personalization is an adapter, not the whole strategy.

The system still needs a general prior. It should work before it has seen months of one user’s life. But personal data makes the last mile much better:

- learns **your** mail client, **your** filesystem, **your** browser layout
- learns **your** office, **your** kitchen, **your** commute
- lowers the generalization burden for repetitive daily workflows
- keeps sensitive training data on-device

The goal is not to replace a general model with a hyper-local one. The goal is to start from a general observer and then adapt it to the user’s environment and routines.

### Revised Stage 1 data pipeline

```text
User works normally
        |
Continuous screen video
        + accessibility snapshots
        + planner action logs / abstract control context
        |
Observer backbone trains on next-observation prediction
conditioned on recent action
        |
Perception heads train on structured scene-state targets
        |
Planner trains on (belief_state, action, observed_outcome)
        |
Localized adaptation to this user’s workflows and environment
```

A crucial correction: **an acting observer needs action context.**
“Never capture any input information” is too strong.
What we want is not raw sensitive keystroke retention. What we want is the smallest safe control trace that lets the system learn causality.

---

## World-Model Architecture

This is still the core research question. The model must:

- compress observations into a compact latent state
- preserve control-critical information, not just novelty
- condition predictions on recent action
- emit features rich enough for typed belief-state heads
- run continuously on local hardware

### Candidate stack

**Spatial encoder:** pretrained vision backbone such as SigLIP-class or Florence-class vision features. Start by inheriting spatial structure; do not train from scratch unless forced.

**Temporal model:** recurrent/state-space model such as Mamba-class sequence modeling or another efficient recurrent alternative. Continuous perception needs long context without transformer-scale quadratic costs on every frame.

**Prediction objective:** predict the next observation in feature space, conditioned on the current latent state and recent action context. Pixel prediction wastes capacity. Semantic feature prediction is the better default.

### Preserve what matters for control

A pure compression objective will happily discard stable details that are essential for control.
The latent must preserve:

- persistent entity identity
- whether a control is enabled, focused, or occluded
- object tracks and egomotion
- free space and collision-relevant geometry
- affordances and contact-relevant state
- uncertainty about all of the above

So the system is not “compress the predictable, keep the surprising.”
It is:

**compress what is redundant while preserving what is necessary for prediction, control, and recovery.**

### Model budget

The hot loop should optimize for:

- active parameters per step
- memory bandwidth
- recurrent state size
- determinism
- latency under sustained load

A dense 100B monolith is not the target architecture for real-time perception + control. The right shape is a **local multi-model stack**:

- small/medium continuous observer
- small/medium planner/controller
- larger reasoning model only off the hot loop when needed

This fits desktops, edge devices, and Jetson-class deployments much better than a single giant model thesis.

---

## Supervision Across Stages

### Stage 1: Accessibility Tree + Action Logs

macOS exposes a rich accessibility tree, but it is sparse, delayed, and imperfect for transient UI states. Polling alone is not enough.

Stage 1 should use a **hybrid capture strategy**:

- low-frequency polling for broad steady-state coverage
- event-driven snapshots after likely UI-mutating actions
- offline alignment to the visual stream
- planner action logs so the observer can learn action-conditioned prediction

The accessibility tree is for training, not runtime. Runtime should assume it is absent.

### Stage 2: Physics + Egomotion + Human Behavior

The physical world gives richer continuity constraints and action-conditioned data. The observer learns not only what the world looks like, but how it changes when the wearer moves and acts.

### Stage 3: Motor Feedback + Contact

The robot closes the loop. Expected and actual physical outcomes diverge. That divergence is dense signal for both observer and planner.

---

## Task Execution Model

Example: “Open my email, find the one from Jack, check if it has the signed contract, show me for confirmation.”

### Reasoning / orchestrator

One reasoning pass decomposes the task into goal steps and watch conditions.

### Planner / controller

The planner executes each step in the closed loop. It uses the observer as a service, not as a prompt.

### Observer interactions

- **publish:** current UI state, search field, result list, attachment affordances
- **query:** “which visible result best matches ‘signed contract’?”
- **watch:** “notify when the attachment viewer is fully loaded”
- **probe_request:** “scroll a little more” or “hover the row to reveal metadata” because confidence is low

### LLM re-engages only when

- the planner cannot recover
- uncertainty remains high after probing
- the task requires judgment or summarization
- the system needs the user to choose between semantically ambiguous options

This is how a language model as orchestrator meshes well with perception networks as tools: typed observer calls below, semantic judgment above.

---

## Deployment Architecture

```text
+--------------------------------------------------------------+
|                        Fawx Harness                          |
|                                                              |
|  +------------------+   +-------------------------------+    |
|  | Reasoning / LLM  |   | Planner / Controller          |    |
|  | goal decomposition|  | actions, retries, recovery    |    |
|  +---------+--------+   +---------------+---------------+    |
|            |                            |                    |
|            | query/watch                | action context     |
|            v                            v                    |
|      +--------------------------------------------------+    |
|      | Safety / Permissions / Commit Boundaries         |    |
|      +----------------------+---------------------------+    |
+-----------------------------|--------------------------------+
                              | observer ABI
                              | publish/query/watch/probe
+-----------------------------|--------------------------------+
|                       fawx-eyes sidecar                      |
|                                                              |
|  spatial encoder -> temporal world model -> heads           |
|                      ^                       |                |
|                      |                       v                |
|                recent action          belief state            |
|                                       uncertainty             |
|                                       novelty                 |
|                                       affordances             |
|                                       probe requests          |
+--------------------------------------------------------------+
```

### Transport

JSON over local IPC is a good debug surface and early integration surface.
It may not be the final high-rate transport.

A likely path:

- **JSON / sockets** for debugging, evaluation, and early versions
- **shared memory / binary transport** for the hot path if frame rate or payload size demands it

### Hardware posture

This should be local-first:

- laptop / desktop GPU for development and early product work
- Jetson-class edge hardware for dedicated local assistants and robots
- larger local boxes for more capable multimodal assistants
- optional remote reasoning for non-real-time tasks only

The observer and planner must remain useful even when the network is absent.

---

## Example Scene-State Contract

This is illustrative, not final:

```json
{
  "profile": "ui/v1",
  "schema_version": "1.0.0",
  "timestamp_ms": 1711234567890,
  "frame_id": 42,
  "belief_state_id": "ui_42_a",
  "prediction_error": 0.41,
  "goal_relevance": 0.92,
  "health": {"observer_ok": true, "latency_ms": 21},
  "uncertainty": {"global": 0.18, "ocr": 0.07, "layout": 0.14},
  "recent_action": {
    "type": "click",
    "target_hint": "search_field",
    "age_ms": 180
  },
  "entities": [
    {
      "id": "el_101",
      "type": "text_field",
      "label": "Search",
      "bounds": {"x": 330, "y": 120, "w": 420, "h": 34},
      "state": {"focused": true, "enabled": true, "visible": true},
      "affordances": ["type", "paste"],
      "confidence": 0.97
    },
    {
      "id": "el_102",
      "type": "list_item",
      "label": "Jack — signed contract",
      "bounds": {"x": 300, "y": 210, "w": 580, "h": 52},
      "state": {"selected": false, "visible": true},
      "affordances": ["open", "hover"],
      "confidence": 0.91
    }
  ],
  "text_regions": [
    {
      "id": "txt_9",
      "text": "2 results",
      "bounds": {"x": 305, "y": 182, "w": 80, "h": 18},
      "confidence": 0.98
    }
  ],
  "watches": [
    {"name": "attachment_loaded", "status": "pending"}
  ],
  "probe_request": {
    "type": "hover",
    "target_id": "el_102",
    "reason": "metadata occluded",
    "priority": "normal"
  },
  "scene_summary": "Mail results visible; likely matching message from Jack with signed contract"
}
```

### Contract versioning

- semver at the schema level
- stable core envelope
- additive changes within a major version
- profile-specific payloads evolve independently from the core envelope when needed

---

## Capture Settings (Revised Bite 1 Assumptions)

### Screen video

- resolution: 1x logical resolution to control storage and training cost
- frame rate: start at 30fps
- codec: H.265 or equivalent efficient local codec
- session management: start/stop, file rotation, disk monitoring

### Accessibility data

- hybrid polling + event-driven snapshots
- timestamped JSONL or equivalent
- aligned offline to frames and action logs
- focused app, window info, element hierarchy, labels, bounds, values where available

### Action / control context

This is now explicitly part of the thesis.

- record abstract action events from the planner
- include action type, target hint, timing, and outcome where known
- avoid retaining raw sensitive text when possible
- secure fields should be redacted or summarized, not stored literally

### Privacy

The privacy boundary remains the device.

Planned controls:

- manual pause/resume
- retention policy with auto-purge for raw captures
- no productized export path for raw captures by default
- training-data wipe surface distinct from AX rollback/ripcord
- encryption at rest and normal OS protections

The policy goal is not “capture nothing.”
The policy goal is **capture the minimum necessary causal trace to train and operate the system safely.**

---

## Program Shape

This is not one spec and not one epic.
It is a **program-level thesis** that should break into multiple epics and then into actual specs.

### Likely epics

1. **Capture + alignment**  
   Screen/camera capture, accessibility snapshots, action logs, storage, retention, alignment.

2. **Observer backbone**  
   Spatial encoder, temporal model, action-conditioned prediction objective, evaluation.

3. **Perception heads + observer ABI**  
   Scene-state schema, uncertainty, tracks, affordances, query/watch/probe APIs.

4. **Sidecar runtime**  
   Local service, transport, resource isolation, profiling, device-specific configs.

5. **Planner / controller**  
   Goal-step execution, retries, stabilization, active-perception handling, safety handoffs.

6. **Evaluation harness**  
   Accuracy, latency, uncertainty calibration, recovery rate, probe utility, user-visible task success.

7. **Privacy / control plane**  
   Pause, purge, retention, consent surfaces, secure field handling.

8. **Stage 2 / 3 transfer**  
   Wearable and robotic heads, egomotion, depth, contact, manipulation affordances.

### First bites

1. **Bite 1:** capture + alignment prototype  
2. **Bite 2:** observer backbone trained on Stage 1 data  
3. **Bite 3:** scene-state contract + sidecar prototype  
4. **Bite 4:** simple planner/controller  
5. **Bite 5:** evaluation + privacy/control plane  

---

## Decisions as of This Revision

1. **The near-term product is a reusable observer sidecar, not a universal perception black box.**
2. **Prediction error is a salience signal, not the full definition of importance.**
3. **Action-conditioned perception is mandatory for any act-perceive-react loop.**
4. **Perception needs limited control via probe requests, but not final actuator authority.**
5. **The LLM belongs above the hot loop as orchestrator and exception handler.**
6. **Perception networks should present as typed tools/services or bounded watchers, not freeform prose subagents.**
7. **The local hot loop should be a multi-model stack, not a dense 100B monolith.**
8. **This document is a parent thesis for multiple epics and specs.**

---

## Open Questions

- What is the smallest safe and privacy-respecting action trace that still teaches causality?
- How much backbone transfer actually survives from Stage 1 screens to Stage 2/3 physical scenes?
- What transport is sufficient for the hot path: JSON, binary, shared memory, or mixed?
- How should uncertainty be calibrated and consumed by the planner?
- When should the observer request probes versus the planner deciding to probe on its own?
- Which parts of the planner stay rule-based longest, and which parts are worth learning first?
- What is the best hardware tier split for laptop, edge box, and robot deployments?

---

## Connection to Existing Fawx Architecture

- **Skills:** the observer is consumed as a skill/service boundary, not as a raw model dump
- **fx-forge:** training infrastructure for the observer backbone, heads, planner, and evaluation
- **AX security model:** planner actions still go through the same capability and permission boundaries
- **Kernel safety:** final actuation remains gated regardless of what the observer or planner wants
- **WASM / sidecars:** lightweight logic may fit in WASM skills; GPU-heavy observer work remains a sidecar

---

*This document captures the architectural thesis as of 2026-04-05. It is not a commitment or a roadmap. It is the direction we are chewing toward, one bite at a time.*
