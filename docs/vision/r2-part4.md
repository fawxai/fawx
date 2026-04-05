
---

## The Progression

The runtime pattern can remain stable across stages. The exact degree of backbone transfer is an empirical question — and this revision makes the decision gate explicit.

### Stage 1: Computer Use

- **Input:** screen capture, window metadata, accessibility snapshots, planner action context
- **Observer:** screen encoder -> action-conditioned temporal model -> UI heads
- **Training supervision:** accessibility tree, OCR targets, event-driven snapshots, action/outcome alignment
- **Planner:** belief state + goal step -> keyboard/mouse/scroll actions
- **Value:** Fawx can use software through perception, not by depending on browser automation or native accessibility APIs at runtime
- **Limitation:** screens are an impoverished causal world. A click can teleport to a visually unrelated state. Physics does not constrain UI transitions.

### Stage 2: Wearable Pendant / Assistive Observer

- **Input:** camera, IMU, optional GPS, optional depth, planner/context signals
- **Observer:** same service shape, different heads and likely some backbone retuning
- **Training supervision:** physics, egomotion, weak supervision from human behavior, optional depth/self-supervision
- **Planner:** belief state + context -> voice alerts, haptics, navigation prompts, probe behaviors
- **Value:** assistance, scene description, text reading, reminders, navigation support

### Stage 3: Robotic Assistant

- **Input:** cameras, proprioception, force/contact, optional depth/LiDAR, planner action context
- **Observer:** spatial world model with object tracks, affordances, contact-aware state
- **Training supervision:** all of Stage 2 plus motor feedback and manipulation outcomes
- **Planner:** belief state + goal -> trajectories, manipulation sequences, recovery behaviors
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

The strong claim is not "one backbone will do all the work."
The stronger and more defensible claim is that the **runtime pattern, training objectives, and observer ABI** can persist while heads and parts of the backbone change.

---

## Stage Transfer Decision Gate

The thesis claims the runtime pattern transfers across stages. The backbone is a separate question, and it must be answered with data, not conviction.

**At the end of Stage 1, before committing to Stage 2, run a transfer evaluation:**

1. Freeze the Stage 1 backbone. Attach fresh Stage 2 perception heads. Train heads only on a small physical-scene dataset.
2. Measure: entity detection mAP, depth estimation error, egomotion accuracy.
3. Compare against:
   - **(a)** same heads trained on a backbone pretrained from scratch on physical scenes
   - **(b)** a standard pretrained vision backbone (e.g. DINOv2) with same heads

**Decision criteria:**

- If Stage 1 backbone + fresh heads scores **within 15% of the scratch-trained baseline** on the core metrics: proceed with shared backbone, fine-tune as needed. The screen-trained features transfer usefully.
- If Stage 1 backbone scores **within 15% of the generic pretrained backbone** but both are far from scratch-trained: use the generic backbone for Stage 2, keep the ABI and runtime pattern. The screen-specific training did not help, but a general visual prior is good enough.
- If Stage 1 backbone is **more than 30% worse than both baselines**: the backbone does not transfer. Fork fawx-eyes into domain-specific observer implementations sharing only the ABI contract and runtime pattern.

**This is a one-time gate, not a continuous decision.** Run it once with sufficient data and commit to the path.

The ABI and runtime pattern are the real bets. Backbone sharing is a nice-to-have. If it works, it saves training compute and simplifies deployment. If it does not, the architecture survives without it.
