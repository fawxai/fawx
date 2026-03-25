# Fawx Perception Engine — Architecture Thesis

**Date:** 2026-03-25
**Authors:** Joe, Clawdio
**Status:** Vision doc — not a spec, not a plan. A thesis to take bites out of.

---

## The Thesis

AI assistants today run everything through an LLM. Every observation, every decision, every action. This is like Tesla sending every camera frame to GPT-4 and asking "what should I do?" It's slow, expensive, and architecturally wrong.

The correct architecture separates perception, planning, and reasoning:

- **Perception** (local, real-time, ~30fps): cameras/screens → structured scene state
- **Planning** (local, fast, ~10fps): scene state + goal → actions
- **Reasoning** (LLM, slow, on-demand): user intent → goal decomposition + judgment calls

The LLM is the executive. The planner is the motor cortex. Perception is the eyes. The executive doesn't decide "move cursor 3px left." It decides "fill in the shipping form," and the planner handles the 50 micro-actions to get there.

---

## The Progression

The architecture is the same at every scale. Only the input sources and action layer change.

### Stage 1: Computer Use
- **Input:** screen capture (single 2D stream)
- **Perception:** UI element detection, OCR, layout understanding
- **Planner:** screen state + goal → keyboard/mouse actions
- **Actions:** input simulation (click, type, scroll, keyboard shortcuts)
- **Value:** Fawx can use any software without accessibility APIs or browser automation

### Stage 2: Wearable Pendant (Assistive)
- **Input:** single camera (pendant/glasses)
- **Perception:** object detection, depth estimation, text reading, person detection
- **Planner:** scene state + context → voice alerts
- **Actions:** speech output, haptic feedback
- **Value:** navigation assistance, scene description, text reading for visually impaired

### Stage 3: Robotic Assistant
- **Input:** stereo cameras, LIDAR, proprioceptive sensors
- **Perception:** full spatial understanding, object manipulation planning
- **Planner:** scene state + goal → motor trajectories + manipulation sequences
- **Actions:** motors, grippers, locomotion
- **Value:** physical personal assistant

---

## Why Personal Training Data Matters

Tesla trained on millions of human drivers doing it right. Not on autopilot stumbling around.

The Fawx planner should be trained on the USER doing things correctly, not on the AI fumbling through tasks.

### Data pipeline:

```
User works normally (Mac, iPhone, etc.)
         ↓
Background capture: (screen_frame, input_action) pairs
         ↓
Perception model extracts structured scene state from each frame
         ↓
Training pairs: (scene_state, action) — labeled by real human behavior
         ↓
Train local planner on THIS USER's patterns
```

### Why this is better than generic training:
- Learns YOUR email client, YOUR file system, YOUR browser layout
- No need to generalize to every possible UI configuration
- Captures workflow patterns (open email from X → download attachment → move to folder Y)
- The pendant version learns YOUR neighborhood, YOUR office, YOUR kitchen
- Privacy: training data never leaves the device

### The prediction horizon:
With enough data, the planner starts anticipating. "You just opened an email from Jack. Last 3 times, you downloaded the attachment and filed it. Want me to do that?" This is earned autonomy through behavioral data — the same signal flywheel architecture as fx-forge and the AX security model.

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
- Step 2: perception sees search field at (x,y) → planner types "from:Jack" → perception confirms results → planner reads results via OCR → matches "contract"
- No LLM call per click. No screenshot-to-cloud per action.

### LLM re-engages only when:
- Step fails and planner can't recover (unexpected state)
- Decision requires judgment ("two emails from Jack — NDA and MSA, which one?")
- Task complete, needs summarization for user

---

## Perception Model Candidates

### For Stage 1 (Computer Use):

**Florence-2 (Microsoft):** Single model, does detection + OCR + segmentation + captioning. Open source, runs on Mac GPU. Best starting point for UI understanding.

**Advantages:**
- Unified model (not a pipeline of separate models)
- Does OCR natively (critical for UI text)
- Bounding box output for clickable elements
- Lightweight enough for local inference

**Alternatives considered:**
- SAM2 + Depth Anything + PaddleOCR: more capable individually, but three models to orchestrate
- YOLOPv3: automotive-focused, heads don't transfer to UI
- VPE (Visual Perception Engine): good architecture paper, but robotics-focused

### For later stages:
The backbone transfers. UI detection heads get swapped for scene understanding heads. The user's training data teaches the new heads. Florence-2's vision transformer backbone is general enough to support this.

---

## Deployment Architecture

```
┌─────────────────────────────────┐
│          Fawx Engine            │
│  (LLM reasoning, skill system) │
│                                 │
│  ┌───────────┐  ┌────────────┐ │
│  │ Skills    │  │ Planner    │ │
│  │ (tools)   │  │ (actions)  │ │
│  └───────────┘  └──────┬─────┘ │
│                         │       │
│         Scene State ◄───┘       │
│             ▲                   │
└─────────────┼───────────────────┘
              │ IPC (local socket)
┌─────────────┼───────────────────┐
│     fawx-eyes (sidecar)        │
│                                 │
│  ┌──────────────────────────┐  │
│  │   Perception Engine      │  │
│  │   (Florence-2 or similar)│  │
│  │                          │  │
│  │   Heads:                 │  │
│  │   - UI element detection │  │
│  │   - OCR / text reading   │  │
│  │   - Layout understanding │  │
│  │   - Depth (future)       │  │
│  └──────────────────────────┘  │
│                                 │
│  Input: screen capture / camera │
│  Output: structured scene state │
│  Rate: 10-30 fps               │
└─────────────────────────────────┘
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
  "timestamp_ms": 1711234567890,
  "frame_id": 42,
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

---

## First Bites

### Bite 1: Background capture (data collection)
- macOS screen capture + input event recording
- Save (frame, action) pairs to disk
- No inference, no model, just data
- Goal: understand the dataset shape and volume
- Timeline: days, not weeks

### Bite 2: Florence-2 scene extraction
- Run Florence-2 on captured frames
- Produce structured scene state from screenshots
- Measure: latency, accuracy on UI elements, OCR quality
- Determine: is Florence-2 good enough, or do we need to fine-tune?
- Timeline: 1-2 weeks

### Bite 3: fawx-eyes sidecar prototype
- Separate binary, local socket IPC
- Publishes scene state stream
- Fawx engine subscribes via a new perception skill
- No planner yet — just "what does Fawx see right now?" as a tool
- Timeline: 1-2 weeks

### Bite 4: Simple planner for computer use
- Given scene state + goal step, output one action
- Start rule-based (if goal is "type X" and field at (x,y), click and type)
- Replace rules with learned planner as training data accumulates
- Timeline: 2-4 weeks

### Bite 5: Training pipeline
- (scene_state, action) pairs from Bite 1 data
- Fine-tune planner on user-specific patterns
- Measure: can the planner predict the next action correctly?
- This is where fx-forge connects — same training infrastructure
- Timeline: 4-8 weeks

---

## Open Questions

1. **Frame rate vs. battery:** Continuous screen capture at 30fps on a MacBook is expensive. What's the minimum useful frame rate? 5fps? Event-driven (capture on input events only)?

2. **Privacy of training data:** Screen recordings contain sensitive content (passwords, messages, financial data). How do we handle this? On-device only? Selective capture? Automatic redaction?

3. **Florence-2 limitations:** How well does it handle macOS-specific UI elements (menu bar, dock, system dialogs)? May need fine-tuning on macOS screenshots specifically.

4. **Planner architecture:** Transformer? Decision tree? RL agent? The answer depends on what the data looks like after Bite 1.

5. **Scene state contract versioning:** As perception improves, the scene state format evolves. Need a stable contract so the planner and engine aren't tightly coupled to one version.

6. **Multi-monitor / multi-app:** User switches between apps constantly. Does the planner maintain state across app switches? Does it need a concept of "focused app"?

---

## Connection to Existing Fawx Architecture

- **Skills:** The perception sidecar is consumed via a skill. The planner could be a skill. The LLM decomposition is the existing agentic loop.
- **fx-forge:** Training pipeline for the planner. Same infrastructure as signal flywheel.
- **AX security model:** Perception data is sensitive. Tripwire boundaries apply. Ripcord can undo actions the planner takes.
- **Kernel safety:** The planner's actions go through the same permission/capability gates as any tool call.
- **WASM skills:** The planner itself could be a WASM skill (if lightweight enough) or a sidecar (if it needs GPU).

---

*This document captures the architectural thesis as of 2026-03-25. It is not a commitment or a roadmap. It's the direction we're chewing toward, one bite at a time.*
