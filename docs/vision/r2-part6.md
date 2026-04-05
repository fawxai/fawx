
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

So the system is not "compress the predictable, keep the surprising."
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

## Uncertainty Calibration

Uncertainty is load-bearing in this architecture. The planner uses uncertainty to decide whether to act, probe, or escalate. If uncertainty estimates are miscalibrated, the whole observe-decide-act loop degrades — either the system hesitates when it should act, or it commits when it should probe.

### Three kinds of uncertainty the observer must provide

1. **Detection uncertainty**: is this entity real and correctly typed? (Is that actually a button, or a decorative element that looks like one?)
2. **State uncertainty**: is the entity's state (enabled, focused, visible, selected) correct? (Is the button actually disabled, or is the visual style ambiguous?)
3. **Semantic uncertainty**: does the label/text match what is actually rendered? (Did OCR read "Submit" or "Submit" with a ligature the model has not seen before?)

Each entity in the belief state carries per-field confidence scores, not just a single global confidence number.

### Calibration metric

The primary calibration metric is **Expected Calibration Error (ECE)**. When the observer says "90% confident," it should be right approximately 90% of the time.

ECE is computed by binning predictions by confidence, computing the accuracy within each bin, and measuring the gap between confidence and accuracy. Lower is better.

**Target:** ECE < 0.15 from Bite 2 onward. This is not a stretch goal. It is a minimum bar.

### How the planner consumes uncertainty

The planner uses confidence thresholds to gate behavior:

- **confidence >= 0.9**: act without hesitation
- **0.7 <= confidence < 0.9**: act but prepare recovery (the planner expects this action may fail and pre-computes a fallback)
- **0.5 <= confidence < 0.7**: probe first if probe budget allows, otherwise act with recovery
- **confidence < 0.5**: escalate to reasoning or skip this target entirely

These thresholds are defaults. They shift based on **action reversibility**:

- Reversible actions (scroll, hover, focus): thresholds shift down (more willingness to act on low confidence)
- Irreversible actions (delete, submit, send): thresholds shift up (require higher confidence before committing)
- The safety layer enforces the irreversibility classification, not the planner

### Miscalibration fallback

Uncertainty estimates that are consistently wrong are worse than no uncertainty at all. A model that says "95% confident" but is right only 60% of the time will cause the planner to skip probing and commit to bad actions.

**Fallback rule:** if ECE exceeds 0.15 on the rolling evaluation window, the observer falls back to **binary high/low confidence** using a fixed threshold on the raw logit, until the model is recalibrated. The planner treats all "low" confidence entities as requiring probing. This is conservative but safe.

Calibration drift is monitored continuously and triggers an alert when it crosses the threshold.
