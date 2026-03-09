# Fleet Knowledge Sharing: Beyond Git for Agent Collaboration

*Inspired by [Andrej Karpathy's autoresearch post](https://github.com/karpathy/autoresearch/discussions/43) — March 2026*

## The Problem

Git assumes eventual convergence to a single canonical state. A fleet of autonomous Fawx instances should **diverge** — each exploring different directions, self-modifying independently, sharing discoveries selectively. Git's "one master branch" model breaks under this.

Karpathy frames this as: the goal isn't one PhD student, it's a research community. Current tools synchronously grow a single thread of commits. What's needed is asynchronous, massively collaborative agent work — SETI@home for AI research.

## Why Git Breaks at Fleet Scale

- **Convergence assumption**: Branches are temporary; they're meant to merge back. A fleet of agents doing parallel autonomous work produces permanent divergence.
- **Conflict resolution is human-shaped**: Merge conflicts assume a human will read both sides and decide. With thousands of branches across hundreds of agents, this doesn't scale.
- **Linear history bias**: Git's DAG technically supports arbitrary topology, but every tool (GitHub, review workflows, CI) assumes linear-ish flow toward one target branch.

## Fawx Self-Modification Categories

Not all artifacts should share a versioning model:

| Category | Example | Sharing model |
|----------|---------|---------------|
| **Skills (WASM plugins)** | `brave-search.wasm` | Package registry (npm/crates.io model) |
| **Knowledge/findings** | "Retry with backoff improves tool call success by 40%" | Structured artifacts, read by other agents |
| **Engine improvements** | Better loop orchestration, new tool executor | Proposal → verify → adopt pipeline |
| **Configuration/personality** | `SOUL.md`, thinking budget | Per-agent, never shared |

## Proposed Architecture

### 1. Skill Marketplace (already planned)

WASM skills are composable and conflict-free. Publish, version, install independently. The npm model works here because skills have defined interfaces and don't conflict — two agents can use different versions of the same skill without merge conflicts.

### 2. Knowledge Federation

Agents share findings as structured artifacts — not code commits. Think ArXiv for agents:

```
Finding {
    agent_id: "fawx-alpha",
    domain: "tool-calling",
    claim: "Exponential backoff on 429s reduces total latency by 35%",
    evidence: { benchmark_id: "...", before: 2.3s, after: 1.5s },
    reproducibility: { config: "...", seed: 42 },
    timestamp: "2026-03-09T15:00:00Z"
}
```

Other agents read the feed, evaluate relevance, and independently decide to test and adopt. No merge. No consensus. Each agent maintains its own knowledge base, informed by the community but not bound to it.

### 3. Improvement Proposals with Fitness Selection

When an agent discovers a core engine improvement:

1. **Publish** — structured proposal with diff, test results, and measured impact
2. **Verify** — other agents independently run the same tests on their own workloads
3. **Adopt** — each agent decides based on its own fitness criteria
4. **Promote** — if N agents adopt and report positive results, the improvement becomes a candidate for the shared base

This is evolutionary selection, not consensus. Modifications that measurably improve outcomes propagate. Those that don't, die naturally.

### 4. The Gossip Protocol

For real-time fleet coordination, something like:

```
- Agent A publishes finding to shared bus
- Agents B, C, D receive it within seconds
- Each evaluates: "Is this relevant to my current task/domain?"
- Relevant agents test locally, report results back to bus
- Results accumulate: "5/7 agents confirmed improvement"
- Agents that haven't adopted yet see the signal strength
```

This is closer to a DHT or gossip network than git. No central authority decides what's canonical. The "truth" emerges from distributed verification.

## Why Fawx's Architecture Enables This

The **kernel/loadable split** is the enabler:

- **Kernel** (immutable at runtime): loop orchestrator, policy engine, proposal gate, safety enforcement. This layer is identical across all fleet members. It's the safety invariant.
- **Loadable** (modifiable): skills, tools, prompts, configuration. This is what agents self-modify.

Because the kernel enforces safety regardless of loadable-layer state, agents can freely share and adopt loadable-layer modifications without risking safety violations. The kernel is the immune system; the loadable layer is the adaptive behavior.

## Karpathy's Interim Hack

His prototype — GitHub Discussions as "papers," PRs as commit trails you never merge — is clever because it uses git as a **transport** while abandoning git's **workflow assumptions**. You get:

- Discoverability (GitHub search, API)
- Exact reproducibility (commit SHAs)
- Discussion threads (peer review)
- No merge pressure (PRs stay open forever)

For SuperFawx v1, this pattern could work: each fleet member maintains its own branch. Discoveries are published as GitHub Discussions with structured metadata. Other agents read via `gh` CLI and decide what to adopt.

## Near-term for SuperFawx

**v1 (practical)**:
- Fleet shares same base binary (deployed from staging)
- Skills shared via marketplace
- Self-modifications stay local to each instance's loadable layer
- A "promotion" mechanism elevates proven modifications to shared base
- Orchestrator handles task routing

**v2 (knowledge sharing)**:
- Structured finding publication
- Cross-agent discovery feed
- Fitness-based propagation
- Reputation/trust layer for modifications

**v3 (full autonomy)**:
- Gossip protocol for real-time sharing
- Evolutionary selection at fleet scale
- Self-organizing research communities
- Agents choose their own research directions based on fleet-wide signal

## The Bigger Insight

> "Existing abstractions will accumulate stress as intelligence, attention and tenacity cease to be bottlenecks." — Karpathy

Git, GitHub, PRs, code review — all designed for humans with limited attention. When agents can juggle thousands of commits across arbitrary branch structures, the bottleneck shifts from "who can review this?" to "how do we select signal from noise at scale?" The tools we build for that selection problem will look nothing like git.
