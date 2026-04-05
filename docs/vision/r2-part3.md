
---

## The Planner / Controller Architecture

The planner received too little attention in earlier revisions. For Stage 1 computer use, planning is at least as hard as perception. A click can cause a page navigation, a modal dialog, a download, a redirect, or nothing. The planner must handle all of these without hardcoded assumptions about any particular application.

### What the planner does

The planner takes a **goal step** from the reasoning layer and a **belief state** from the observer, and produces **actions** to advance toward the goal. It is a closed-loop controller, not a one-shot action predictor.

Its responsibilities:

1. **Action selection**: choose the next action given current belief state and goal step
2. **State transition verification**: confirm the expected transition happened after acting
3. **Stabilization**: wait for the environment to settle before re-observing or acting again
4. **Retry and recovery**: detect failures and attempt recovery before escalating
5. **Probe arbitration**: execute or deny observer probe requests, enforce the probe budget
6. **Safety coordination**: submit proposed actions to the safety layer before execution

### Planner state machine

The planner for a single goal step runs a state machine:

```
OBSERVE ---> DECIDE ---> SAFETY CHECK ---> ACT ---> STABILIZE ---> VERIFY
  ^            |                                                      |
  |            | probe_request                                        |
  |            | (if budget > 0)                                      |
  |            v                                                      |
  +------<--- PROBE                                                   |
  |                                                                   |
  +------<-------------- mismatch (retry <= N) -----------------------+
                                                                      |
                                                              DONE / ESCALATE
```

**OBSERVE**: receive fresh belief state from the observer.

**DECIDE**: choose action or honor a probe_request. If probe budget allows and the observer requests a probe, execute it and return to OBSERVE.

**SAFETY CHECK**: submit the proposed action to the safety/permissions layer. If denied, return to DECIDE with a constraint.

**ACT**: execute the approved action (keystroke, click, scroll, voice command, motor command).

**STABILIZE**: wait for the environment to settle (see Stabilization below).

**VERIFY**: check that the expected state transition occurred. If it did, the step is DONE. If not and retries remain, return to OBSERVE. If retries are exhausted, ESCALATE to reasoning.

### Stabilization

Stabilization is the hardest mundane problem in computer use. After a click, the screen may:

- change immediately
- animate for 200ms
- load for 3 seconds
- show a spinner then replace content
- do nothing (the click missed or the target was disabled)
- pop a modal that blocks everything else

The planner cannot just wait a fixed time. It must use the observer's belief state to detect when the environment has settled:

- **prediction error drops below baseline** after a post-action spike -> environment is stable
- **no entity changes for N consecutive frames** -> environment is stable
- **a loading indicator is present and tracked** -> wait until it disappears or timeout
- **timeout** -> either the action had no visible effect (possible miss) or the environment is stuck

Stabilization parameters should be tunable per profile and per application class. A terminal stabilizes in milliseconds. A web app may take seconds. The planner should learn these timings from experience rather than hardcoding them.

**Default stabilization budget:** 5 seconds ceiling per action. If the environment has not settled by then, the planner treats the current state as stable and proceeds to VERIFY. This ceiling is configurable and should be learned over time.

### Retry and recovery

When verification fails, the planner has a retry budget per goal step (default: 3, configurable). Each retry returns to OBSERVE and re-enters the loop.

Recovery strategies, in priority order:

1. **Re-attempt the same action** — maybe the click was slightly off, the element moved, or timing was unlucky
2. **Adapt the action** — click a different coordinate for the same target, use keyboard instead of mouse, scroll to reveal the target
3. **Back out and retry the step** — press Escape, navigate back, close a modal that appeared unexpectedly
4. **Escalate to reasoning** — the planner cannot recover. The LLM re-evaluates the goal decomposition or asks the user

When retries are exhausted without recovery, the planner **must escalate**. It should not loop silently.

### What stays rule-based vs. what gets learned

The planner will likely remain substantially rule-based in early stages:

**Rule-based longest:**
- safety checks and permission gates
- stabilization timeout ceilings
- retry budget enforcement
- the state machine structure itself

**Worth learning early:**
- stabilization timing predictions per app context
- action selection (which element to target, which input method to use)
- recovery strategy ranking (which recovery is most likely to succeed given the failure mode)
- state transition prediction (what should the observer expect to see after this action?)

The learned components should have rule-based fallbacks. If the learned model is uncertain, the planner falls back to conservative defaults (longer stabilization, simpler actions, earlier escalation).

### Planner candidate stack

**Stage 1 (initial):** rule-based state machine with heuristic stabilization and fixed retry strategies. The observer does the hard perceptual work; the planner is a simple reactive controller. This is fast to build and easy to debug.

**Stage 1 (learned):** lightweight policy model trained on (belief_state, goal_step, action, outcome) tuples. Can be as small as a few-million-parameter MLP or small transformer over the belief state sequence. Trained offline on logged episodes. The rule-based planner generates the initial training data.

**Stage 2/3:** the planner becomes more capable as the action space grows. Physical actions require trajectory planning, not just discrete UI events. The state machine structure persists but the action selection and recovery components need substantially more capacity.
