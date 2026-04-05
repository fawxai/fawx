
---

## The Observer ABI

The observer should be reusable, but only for harnesses that implement the interface. The right abstraction is not "plug this into any harness and it works." The right abstraction is:

**A typed observer ABI with a stable core envelope and domain-specific profiles.**

### Core envelope

Every observer message should carry the same core fields:

- timestamp / frame id / sensor ids
- current profile and schema version
- prediction error / novelty estimate
- uncertainty and health (see Uncertainty Calibration below)
- recent action context
- goal relevance, if available
- belief state id / persistent track ids
- optional probe request with remaining probe budget

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

This makes perception usable as a tool and, where useful, as a bounded watcher. It avoids turning every detector and tracker into a prose-speaking "subagent."

### Probe budget and circuit breaker

The observe-probe-observe loop is powerful but can oscillate. If the observer keeps requesting probes and confidence does not improve, the system wastes time and actions.

**Rules:**

- Each goal step begins with a **probe budget** (default: 3 probes per step, configurable per profile).
- Each probe_request decrements the budget. The remaining budget is visible in the belief state envelope.
- When the budget reaches zero, no further probes are issued for that step. The planner must either **act on current uncertainty**, **escalate to reasoning**, or **fail the step**.
- If two consecutive probes of the same type against the same target do not reduce the relevant uncertainty estimate by at least a configurable threshold (default: 10% relative), the circuit breaker trips early and the budget is treated as exhausted.
- The reasoning layer can grant additional budget if it determines the step is worth more exploration. This is the only way to refill.
- Probe budget consumption is logged. Chronic budget exhaustion is a signal that either the observer needs retraining or the task decomposition is too aggressive.

This prevents spin loops while preserving the observer's ability to request what it genuinely needs.
