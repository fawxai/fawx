# Fawx Perception Engine — Architecture Thesis

**Date:** 2026-03-25, revised 2026-04-05, revised 2026-04-05-r2  
**Authors:** Joe, Clawdio, Fawx  
**Status:** Vision doc — architecture thesis for a multi-epic program. Not a spec, not a plan, not a roadmap. A direction to take bites out of.

---

## What Changed in This Revision (r2)

This revision incorporates corrections from r1 and extends the thesis where it was thin:

- **Surprise is a salience signal, not the whole salience function.** (r1)
- **Perception for acting systems must be action-conditioned.** (r1)
- **The output is a typed belief state, not just detections.** (r1)
- **This is not yet a universal plug-in engine for any harness.** (r1)
- **This is a local multi-model architecture, not a 100B monolith thesis.** (r1)
- **The planner/controller now has an architecture section at parity with the observer.** The planner is at least as hard as perception for Stage 1, and the previous revision left it underspecified. (r2)
- **Evaluation criteria are now concrete and attached to bites.** Each bite has exit criteria. The eval harness is not a deferred problem. (r2)
- **The privacy section now names threat models and defines "minimum necessary."** (r2)
- **Stage transfer has an explicit decision gate with kill criteria.** The thesis now says what happens if backbone transfer fails. (r2)
- **Uncertainty calibration is load-bearing and gets its own treatment.** (r2)
- **Probe requests have a budget and a circuit breaker.** The observe-probe-observe loop cannot spin indefinitely. (r2)

---

## The Core Thesis

Signal is partly surprise. Noise is predictability.

A trained world model processes a sensory stream and its prediction error is a powerful salience signal. If the model predicts the next observation correctly, nothing novel happened. If prediction error spikes, something changed that the model did not expect. That is useful.

But novelty is not the same thing as importance.

A flashing notification may be surprising and irrelevant. A pedestrian steadily approaching a curb may be predictable and critically important. A disabled button that has been disabled for ten seconds may be the only thing that matters for the current task. So prediction error is not "attention for free." It is one free signal inside a larger attention system.

The perception engine's job is not "detect every element on screen" or "label every object in a room." It is:

**Given the current sensory field, recent action, and active goal context, produce the smallest typed belief state that preserves what matters now.**

That belief state includes not just entities and text, but also uncertainty, affordances, and whether the observer needs a better view before the planner should commit.

The compressed latent state remains central. But the latent is not the product. The product is the observer state derived from it.

---

## The Architecture

AI assistants today overuse the LLM. They try to route too much observation, too many decisions, and too many micro-actions through one text model. That is slow, expensive, and architecturally wrong.

The right architecture separates continuous observation, closed-loop control, and slow reasoning, while coupling them explicitly:

- **Observer / Perception** (local, continuous, ~30fps): sensor stream + recent action context -> latent world state -> typed belief state
- **Planner / Controller** (local, fast, ~10fps): belief state + goal step -> actions, retries, recovery, stabilization
- **Reasoning / Orchestrator** (local or remote, slow, on-demand): user intent -> decomposition, memory use, judgment calls, exception handling
- **Safety / Permissions** (always in the loop): capability gates, commit boundaries, tripwires, policy checks

The coupling matters:

- The **planner sends recent action context** to perception so the observer can distinguish self-caused change from unexpected change.
- The **observer sends belief state, uncertainty, novelty, and affordances** back to the planner.
- The **observer may request probes** such as hover, scroll, pan, slow down, reobserve, or zoom. Probe requests carry a budget and the planner enforces a circuit breaker (see Probe Budget below).
- The **planner and safety layer retain final actuation authority**. Perception can ask for a better observation. It should not own the final motor command in a safety-critical loop.
- The **LLM stays above the event horizon**. It should not schedule detectors frame-by-frame or micromanage tensors.

The LLM is the executive. The planner is the motor cortex. Perception is the observer. Safety is the nervous system that says "not that action, not yet."

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
