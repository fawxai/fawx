
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

Example: "Open my email, find the one from Jack, check if it has the signed contract, show me for confirmation."

### Reasoning / orchestrator

One reasoning pass decomposes the task into goal steps and watch conditions.

### Planner / controller

The planner executes each step in the closed loop. It uses the observer as a service, not as a prompt.

### Observer interactions

- **publish:** current UI state, search field, result list, attachment affordances
- **query:** "which visible result best matches 'signed contract'?"
- **watch:** "notify when the attachment viewer is fully loaded"
- **probe_request:** "scroll a little more" or "hover the row to reveal metadata" because confidence is low

### LLM re-engages only when

- the planner cannot recover
- uncertainty remains high after probing
- the task requires judgment or summarization
- the system needs the user to choose between semantically ambiguous options

This is how a language model as orchestrator meshes well with perception networks as tools: typed observer calls below, semantic judgment above.
