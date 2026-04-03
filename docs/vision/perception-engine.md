# Fawx Perception Engine — Architecture Thesis

**Date:** 2026-03-25, revised 2026-03-26
**Authors:** Joe, Clawdio
**Status:** Vision doc — not a spec, not a plan. A thesis to take bites out of.

---

## The Core Insight

Signal is surprise. Noise is predictability.

A trained world model processes a visual stream and its prediction error *is* attention. If the model predicts the next frame correctly, nothing interesting happened — discard. If prediction error spikes, something changed that the model didn't expect — that's signal. You don't need to engineer attention. Prediction error gives it to you for free.

This is how biological perception works. Your visual cortex is a prediction engine. You don't "see" your desk. Your brain predicts your desk and only notices when something violates the prediction. The entire sensory pipeline filters out the predictable and escalates the surprising.

The perception engine's job is not "detect every element on screen" or "label every object in a room." It's: **given this visual field, what matters right now?**

Once the model is trained, we only need to preserve state to the extent of what the current world model tokenizes to. Frames stream through and get discarded. The compressed latent state *is* the perception.

---

## The Architecture

AI assistants today run everything through an LLM. Every observation, every decision, every action. This is like Tesla sending every camera frame to GPT-4 and asking "what should I do?" It's slow, expensive, and architecturally wrong.

The correct architecture separates perception, planning, and reasoning:

- **Perception** (local, real-time, ~30fps): visual stream → compressed world state → structured scene state
- **Planning** (local, fast, ~10fps): scene state + goal → actions
- **Reasoning** (LLM, slow, on-demand): user intent → goal decomposition + judgment calls

The LLM is the executive. The planner is the motor cortex. Perception is the eyes. The executive doesn't decide "move cursor 3px left." It decides "fill in the shipping form," and the planner handles the 50 micro-actions to get there.

### Inside fawx-eyes: Three Layers

The perception engine is not one model. It's three layers stacked:

1. **Video tokenizer** — compresses raw frames into compact latent tokens via next-frame prediction. Learns what's predictable (noise) vs surprising (signal). Self-supervised; needs only video.

2. **Embeddings** — the latent tokens are semantically meaningful and temporally stable. "I've seen this UI state before" and "this scene is similar to one where the user did X." Connects perception to memory and recall.

3. **Perception head** — takes the tokenized, embedded representation and produces structured scene state JSON. Elements, labels, bounding boxes, text regions, scene summaries. This is the reactive layer — it doesn't just compress, it *interprets*.

---

## The Progression

The architecture is the same at every scale. Only the input sources, action layer, and training supervision change.

### Stage 1: Computer Use
- **Input:** screen capture (single 2D video stream)
- **Perception:** video tokenizer → perception head → UI elements, OCR, layout
- **Training supervision:** accessibility tree provides ground-truth labels (element types, names, bounding boxes, text values)
- **Planner:** scene state + goal → keyboard/mouse actions
- **Actions:** input simulation (click, type, scroll, keyboard shortcuts)
- **Value:** Fawx can use any software without accessibility APIs or browser automation
- **Limitation:** This is the *impoverished* training environment. Screens are arbitrary, stateless, and semantically opaque without the accessibility tree. There's no physics, no spatial continuity, no object permanence. A button click can teleport to a completely unrelated visual state.

### Stage 2: Wearable Pendant (Assistive)
- **Input:** single camera (pendant/glasses), IMU, GPS
- **Perception:** same tokenizer + embeddings, retrained perception head for 3D scenes
- **Training supervision:** physics replaces the accessibility tree. Depth from stereo/monocular structure-from-motion gives ground-truth geometry. IMU/ego-motion gives action-consequence pairs (turn left → scene rotates right). Physics gives temporal consistency (objects fall, doors swing, liquids pour). The pendant wearer's behavior (where they walk, look, reach, avoid) *is* the semantic label — the human is the accessibility tree.
- **Planner:** scene state + context → voice alerts
- **Actions:** speech output, haptic feedback
- **Value:** navigation assistance, scene description, text reading for visually impaired

### Stage 3: Robotic Assistant
- **Input:** stereo cameras, LIDAR, proprioceptive sensors
- **Perception:** same tokenizer + embeddings, full spatial perception head
- **Training supervision:** all of Stage 2 plus proprioceptive feedback (motor positions, force sensors, contact detection). The richest training environment.
- **Planner:** scene state + goal → motor trajectories + manipulation sequences
- **Actions:** motors, grippers, locomotion
- **Value:** physical personal assistant

### What transfers across stages:
- **Video tokenizer** — next-frame prediction is universal. Compress the predictable, escalate the surprising. The objective doesn't change.
- **Embedding space** — temporal consistency and similarity structure. "I've seen a state like this before" works on screens and in kitchens.
- **Signal-from-noise separation** — the core skill. Domain changes, objects change, action space changes. The ability to filter noise and attend to signal is the invariant.

### What doesn't transfer:
- **Perception head** — structured output changes per stage. UI elements are not physical objects are not manipulation targets. The head retrains; the backbone carries forward.
- **Training supervision** — accessibility tree is Stage 1 only. Physics replaces it in Stages 2-3 and is a *richer* supervisor. Stage 1 is actually the impoverished training environment — we start there because it's convenient, not because it's easier.

### Why physics is a richer supervisor than the accessibility tree

The accessibility tree is a crutch we need *because* screens are arbitrary. A button can be any color, any size, anywhere, do anything. There's no physics constraining UI layout. So someone has to declare "this is a button here."

The physical world supervises itself:
- **Depth** is free geometry — ground-truth 3D structure from cameras and sensors
- **Ego-motion** is free action-consequence pairing — IMU tells you how the camera moved, the visual change confirms it
- **Physics** is free temporal supervision — gravity, collision, permanence, fluid dynamics. The model predicts the next frame, physics determines what happens, the delta is the loss
- **Object permanence** is free — walk past a chair, circle back, still there. Screens actively violate this (tabs disappear, pages navigate away)
- **Human behavior** replaces semantic labels — you don't need to label "door." You observe the human approach a surface, grasp a lever, pull, walk through. The action sequence *is* the label

---

## Why Personal Training Data Matters

Tesla trained on millions of human drivers doing it right. Not on autopilot stumbling around.

### Data pipeline:

```
User works normally (Mac)
         |
Background capture: continuous H.265 screen video
         + periodic accessibility tree snapshots as labels
         |
Video tokenizer learns to predict next frame (self-supervised)
         |
Perception head learns to output structured scene state
         (supervised by accessibility tree labels)
         |
Planner trains on (scene_state, action, observed_outcome) triples
         |
         Action labels: TBD — options include inverse dynamics from
         visual diffs, replay of agentic-loop action logs, or
         reintroducing lightweight input capture for Bite 4+.
         This is an open design question for the planner stage.
         |
Personalized to THIS USER's visual environment
```

### Why this is better than generic training:
- Learns YOUR email client, YOUR file system, YOUR browser layout
- No need to generalize to every possible UI configuration
- Captures visual patterns specific to your workflow
- The pendant version learns YOUR neighborhood, YOUR office, YOUR kitchen
- Privacy: training data never leaves the device

### The prediction horizon:
With enough data, the tokenizer starts anticipating visual state. Prediction error drops for routine workflows — the model *expects* the next screen. When something unexpected appears (an error dialog, an unusual notification), prediction error spikes — that's the attention signal. The planner inherits this: "this is a familiar workflow, proceed" vs "something unexpected happened, escalate."

---

## Video Tokenizer Architecture

This is the core research question. The architecture needs to: compress frames into compact latent tokens, capture temporal dynamics across frames, and provide features rich enough for the perception head to extract structured scene state.

### Three-layer stack:

**Spatial encoder (frozen):** Pretrained vision backbone — SigLIP-Large or the Florence-2 vision transformer. Takes a frame, outputs a grid of patch embeddings. A 2560x1440 frame with 14x14 patches produces roughly 18,800 spatial tokens of 1152 dimensions. This layer is frozen or lightly fine-tuned. We inherit spatial understanding; we don't train it from scratch.

**Temporal model (trained):** Takes sequences of frame embeddings, learns what's predictable across time, compresses away the redundant. Architecture: **Mamba (state-space model).** Linear scaling with sequence length, handles long contexts efficiently. Screen content has long stretches of near-static video punctuated by sudden changes. Mamba's selective state space is architecturally suited to "carry forward slowly, react to change fast."

Alternative considered: sliding-window transformer. Better at precise temporal dependencies, but quadratic cost makes continuous video processing expensive. Mamba is the starting point.

**Prediction objective:** Predict the next frame's *embedding*, not pixels. This is critical. Pixel-level prediction wastes capacity on irrelevant visual noise (subpixel antialiasing, cursor blink, clock ticking). Embedding-level prediction captures semantic surprise: "a dialog appeared" vs "the cursor moved 2 pixels." Prediction error in embedding space is the attention signal.

### Latent dimensionality:

The Mamba hidden state is the compressed world state — everything the model knows about the current visual scene.

- Model dimension: 1024
- State expansion: 16x
- 4 layers: ~65K floats, roughly **256KB of world state**

For comparison: a human retina sends ~10 Mbps to the visual cortex. By V4/IT (where object recognition happens), that's compressed to ~100KB of active neural state. 256KB for a screen with ~200 UI elements is in the right ballpark.

This gets tuned empirically. Too small and perception quality drops, the model forgets elements between frames. Too large and capacity is wasted, inference slows. The right size is the smallest latent that lets the perception head produce accurate scene state. Measurable criterion.

---

## Supervision Across Stages

### Stage 1: Accessibility Tree (structured labels, free on macOS)

macOS exposes the full accessibility tree for every application: button labels, text field values, menu items, window hierarchy, element roles, bounding boxes. For any given moment we can query the OS for structured UI state — high-quality ground truth, though not literally every transient state.

The accessibility tree is a **training-time luxury**. We use it while we have it so the model doesn't need it when we don't. At runtime: gone. On a pendant: never existed. On a robot: doesn't apply. But during Stage 1 training, it's the labeled dataset that teaches the model to produce structured scene state.

- Capture: separate thread polls accessibility tree at some cadence (starting point: 1-2 Hz)
- Output: timestamped JSONL, aligned to video frames offline
- **Open question: optimal polling cadence.** 1 Hz likely captures most steady-state UI, but transient states (menus, dialogs, mid-typing) will be missed at low rates. The agentic loop currently snapshots the tree after every UI-mutating action, which is event-driven rather than polled. The right answer may be hybrid: low-frequency polling + event-triggered snapshots during active interaction. This needs empirical testing.
- The visual stream is continuous regardless — polling gaps mean noisier labels, not missing training data

### Stage 2: Physics + Human Behavior

The physical world supervises itself. No labels needed:

- **Depth** gives ground-truth 3D geometry without annotation
- **Ego-motion** gives supervised action-consequence pairs via IMU
- **Physics** gives temporal supervision via prediction error against physical reality
- **Object permanence** gives consistency constraints the model must learn
- **Human behavior** gives semantic labels implicitly — the wearer's actions teach the model what things *do* without anyone declaring what they *are*

### Stage 3: Motor Feedback (closed-loop)

The robot acts and observes consequences. Grasp an object and feel resistance (or don't). Push a door and it swings (or doesn't). The prediction error between expected and actual physical outcome is dense, continuous training signal.

---

## Task Execution Model

Example: "Open my email, find the one from Jack, check if it has the signed contract, show me for confirmation."

### LLM decomposes (one call, ~2s):
```
1. Open email client
2. Search for recent emails from "Jack"
3. Find email with attachment matching "contract" or "docusign"
4. Open the attachment
5. Extract: signer names, signature status, date
6. Present summary to user
7. Wait for confirmation
```

### Planner executes each step at frame rate:
- Step 2: perception sees search field at (x,y), planner types "from:Jack", perception confirms results, planner reads results via OCR, matches "contract"
- No LLM call per click. No screenshot-to-cloud per action.
- When prediction error spikes (unexpected dialog, empty results), the planner knows something unexpected happened and can escalate.

### LLM re-engages only when:
- Step fails and planner can't recover (unexpected state / high prediction error)
- Decision requires judgment ("two emails from Jack — NDA and MSA, which one?")
- Task complete, needs summarization for user

---

## Deployment Architecture

```
+-------------------------------------+
|          Fawx Engine                |
|  (LLM reasoning, skill system)     |
|                                     |
|  +-----------+  +--------------+   |
|  | Skills    |  | Planner      |   |
|  | (tools)   |  | (actions)    |   |
|  +-----------+  +------+-------+   |
|                         |           |
|         Scene State <---+           |
|             ^                       |
+-------------|---------------------  +
              | IPC (local socket)
              | Scene state JSON (~1-5KB/frame)
+-------------|---------------------  +
|     fawx-eyes (sidecar)            |
|                                     |
|  +------------------------------+  |
|  |   Video Tokenizer            |  |
|  |   (SigLIP -> Mamba)          |  |
|  |         |                    |  |
|  |   Embeddings                 |  |
|  |   (temporal latent state)    |  |
|  |   ~256KB world state         |  |
|  |         |                    |  |
|  |   Perception Head            |  |
|  |   (structured output)        |  |
|  |                              |  |
|  |   Stage 1 heads:             |  |
|  |   - UI element detection     |  |
|  |   - OCR / text reading       |  |
|  |   - Layout understanding     |  |
|  |                              |  |
|  |   Stage 2 heads (future):    |  |
|  |   - Object detection         |  |
|  |   - Depth estimation         |  |
|  |   - Spatial mapping          |  |
|  +------------------------------+  |
|                                     |
|  Input: screen capture / camera     |
|  Output: structured scene state     |
|  Latent: ~256KB world state         |
|  Rate: 10-30 fps                   |
+-------------------------------------+
```

**Sidecar, not built-in.** Reasons:
- Keeps Fawx engine clean (no ML framework dependency in Rust core)
- Perception model can be swapped/upgraded independently
- Different devices get different sidecar configs (Mac GPU vs phone NPU vs robot GPU)
- Can run in a separate process/container for resource isolation
- Same pattern as fawx-tui (separate binary, communicates with engine)

**Scene state contract (JSON over IPC):**
```json
{
  "version": "1.0",
  "timestamp_ms": 1711234567890,
  "frame_id": 42,
  "prediction_error": 0.73,
  "elements": [
    {
      "type": "button",
      "label": "Submit",
      "bounds": {"x": 340, "y": 520, "w": 120, "h": 40},
      "confidence": 0.97
    },
    {
      "type": "text_field",
      "label": "Email",
      "bounds": {"x": 340, "y": 460, "w": 300, "h": 36},
      "value": "",
      "confidence": 0.94
    }
  ],
  "text_regions": [
    {
      "text": "Error: email is required",
      "bounds": {"x": 340, "y": 500, "w": 200, "h": 16},
      "style": "error"
    }
  ],
  "scene_summary": "Form with empty email field, submit button, error message"
}
```

**Contract versioning:** Semver. Additive-only changes within a major version — new fields appear, old fields never disappear or change meaning. Planner and engine code against a declared minimum version.

Note the `prediction_error` field. This is the tokenizer's surprise signal exposed to the planner. High values mean something unexpected appeared in the visual field. The planner can use this to decide when to escalate to the LLM.

---

## Capture Settings (Bite 1)

### Screen video:
- Resolution: 1x logical (2560x1440, not Retina backing pixels). Text is still readable. Halving each dimension is a 4x data reduction.
- Frame rate: 30fps. Screen content is mostly static so H.265 compresses it aggressively (P-frames are nearly free when nothing moves).
- Codec: H.265, CRF 22 (visually lossless range — text stays sharp, gradients stay clean). CRF 28+ introduces blocking artifacts around text. CRF 18 and below is overkill.
- Pixel format: YUV 4:2:0 (screen content doesn't need chroma fidelity — UI colors are flat, text is high-contrast).
- Volume estimate: ~2-4 Mbps typical → 1-2 GB/hour, 8-16 GB/workday, 2-4 TB/year. A 4TB external SSD handles a full year.

### Accessibility tree:
- Polling rate: TBD — starting point 1-2 Hz, likely needs event-triggered augmentation (see open question in Supervision section)
- Format: timestamped JSONL
- Alignment: offline against video frame timestamps (tolerance TBD with cadence)
- Includes: focused app, window title, element hierarchy with types/labels/bounds/values

### Privacy:
The privacy boundary is the device. This data never leaves the machine. Training is local. The raw video exists to produce model weights, then gets purged. The model learns temporal patterns and visual structure — it doesn't memorize specific frames. A next-frame prediction model that overfits to individual frames is a broken model; generalization pressure works in privacy's favor.

Planned controls (none of these exist yet — all are TODO for the fawx-eyes control plane):
- Manual pause/resume (user hits a key to stop recording during sensitive work)
- Retention policy: raw video auto-deletes after N days, only model weights persist
- No export mechanism for raw capture data — it's a training input, not a product feature
- Training data wipe surface: needs its own implementation. The AX ripcord currently handles file/git rollback for agentic actions but does not cover training data purge — that's a separate control to build.
- FileVault handles the physical-access threat model

### What we don't capture:
- No input events (keystrokes, mouse clicks). The model learns from visual consequences, not input actions. You typed a password — it sees dots appear in a field. It doesn't need to know what you typed.
- No audio.
- Primary display only. Multi-monitor adds complexity for marginal training value. Add later if needed.

---

## First Bites

### Bite 1: Screen Video Capture
- macOS screen recording daemon. H.265 via ffmpeg/AVFoundation.
- Separate thread polls accessibility tree at 1-2 Hz, writes timestamped JSONL.
- Session management: start/stop, file rotation, disk monitoring.
- No inference, no model, just data collection.
- One permission required: Screen Recording. (Accessibility permission for the a11y tree.)
- Goal: accumulate a training dataset. Understand volume, compression ratios, accessibility tree coverage.
- Timeline: days, not weeks.

### Bite 2: Video Tokenizer + Perception Head
Two parallel workstreams:
- **Video tokenizer:** train a model (frozen SigLIP encoder → Mamba temporal model) on screen recordings via next-frame embedding prediction. This learns to compress visual state into compact latent tokens and implicitly learns signal vs noise.
- **Perception head:** train a structured output head supervised by accessibility tree labels. Input: latent tokens from the tokenizer. Output: the scene state JSON contract. This is where the accessibility tree earns its keep.
- Florence-2 may contribute a pretrained vision backbone, but it's a feature extractor candidate, not the model.
- Timeline: 1-2 weeks per workstream, running in parallel with Bite 1 data collection.

### Bite 3: fawx-eyes Sidecar Prototype
- Separate binary, local socket IPC.
- Runs the three-layer stack internally: tokenizer → embeddings → perception head.
- Publishes scene state stream over IPC.
- Fawx engine subscribes via a new perception skill.
- No planner yet — just "what does Fawx see right now?" as a tool.
- Timeline: 1-2 weeks.

### Bite 4: Simple Planner for Computer Use
- Given scene state + goal step, output one action.
- Start rule-based (if goal is "type X" and field at (x,y), click and type).
- Prediction error from the tokenizer gives the planner free anomaly detection — it doesn't need to detect surprises, just respond to them.
- Replace rules with learned planner as training data accumulates.
- Timeline: 2-4 weeks.

### Bite 5: Training Pipeline
Multi-objective training:
1. Next-frame embedding prediction → tokenizer (self-supervised, needs only video)
2. Frame → accessibility tree mapping → perception head (supervised, Stage 1 only)
3. Embedding consistency → temporal coherence of the latent space
4. Planner training comes after 1-3 are working

The pipeline must be designed so that **objectives 1 and 3 transfer to Stages 2 and 3 unchanged.** Only objective 2 is Stage 1-specific (accessibility labels). In Stage 2, depth/IMU/physics replace it. The tokenizer and embeddings carry forward.

This is where fx-forge connects — same training infrastructure, same signal flywheel.
- Timeline: 4-8 weeks.

---

## Resolved Questions

1. **Capture settings:** 2560x1440 @ 30fps, H.265 CRF 22, YUV 4:2:0. ~1-2 GB/hour. 4TB SSD for a year.

2. **Privacy:** Device-local only. Auto-purge raw video after N days. No input event capture — model learns from visual consequences. FileVault for physical access.

3. **Video tokenizer architecture:** Frozen SigLIP encoder → Mamba temporal model → embedding-space prediction loss. Mamba chosen for linear scaling and "carry forward slowly, react to change fast" fit with screen content.

4. **Planner architecture:** Deferred to Bite 4. The planner consumes scene state JSON (text), not video. Simplest version is the existing LLM with scene state as context. Fine-tuned small model replaces it as data accumulates.

5. **Scene state contract versioning:** Semver. `version` field in JSON. Additive-only within a major version.

6. **Multi-monitor:** Primary display only. Add later if the planner needs cross-display context.

7. **Accessibility tree sync:** Separate thread polls at 1-2 Hz. Timestamped JSONL. Offline alignment to video frames at +/- 500ms.

8. **Latent dimensionality:** Mamba hidden state: model dim 1024, state expansion 16, 4 layers = ~256KB world state. Tune empirically — smallest latent that lets perception head produce accurate scene state.

---

## Connection to Existing Fawx Architecture

- **Skills:** The perception sidecar is consumed via a skill. The planner could be a skill. The LLM decomposition is the existing agentic loop.
- **fx-forge:** Training pipeline for the tokenizer, perception head, and planner. Same infrastructure as signal flywheel.
- **AX security model:** Perception data is sensitive. Tripwire boundaries apply. Ripcord can undo actions the planner takes and wipe training data.
- **Kernel safety:** The planner's actions go through the same permission/capability gates as any tool call.
- **WASM skills:** The planner itself could be a WASM skill (if lightweight enough) or a sidecar (if it needs GPU).

---

*This document captures the architectural thesis as of 2026-03-26. It is not a commitment or a roadmap. It's the direction we're chewing toward, one bite at a time.*
