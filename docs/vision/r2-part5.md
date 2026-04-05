
---

## Why Physics Is a Richer Supervisor Than the Accessibility Tree

The accessibility tree is a training-time luxury. It exists because screens are arbitrary and underconstrained. A button can be any color, any size, anywhere, and mean anything. Someone has to declare "this is a button."

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

The system still needs a general prior. It should work before it has seen months of one user's life. But personal data makes the last mile much better:

- learns **your** mail client, **your** filesystem, **your** browser layout
- learns **your** office, **your** kitchen, **your** commute
- lowers the generalization burden for repetitive daily workflows
- keeps sensitive training data on-device

The goal is not to replace a general model with a hyper-local one. The goal is to start from a general observer and then adapt it to the user's environment and routines.

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
Localized adaptation to this user's workflows and environment
```

A crucial correction: **an acting observer needs action context.**
"Never capture any input information" is too strong.
What we want is not raw sensitive keystroke retention. What we want is the smallest safe control trace that lets the system learn causality.
