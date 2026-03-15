# Cloud Launch Strategy

## Overview

This document captures the strategy for training and hosting a Fawx-specific LLM on cloud infrastructure, eliminating dependency on rate-limited cloud APIs while maintaining Opus-level capability for Fawx's operational domain.

## Context

### The Problem

Cloud models like Opus 4.6 are the most capable LLMs available, but:

- **Rate limits** prevent sustained autonomous execution (e.g., building an OS, multi-day operations)
- **Cost scales linearly** with token usage — autonomous operation burns tokens without human oversight
- **Network latency** (~500ms-2s per call) slows down routine decisions
- **Infrastructure dependency** — outages, policy changes, and deprecations are outside our control

### Why Not Just Use an Unlimited API Key?

An enterprise/scale tier API key with uncapped rate limits solves the rate limit problem but:

- **Cost becomes the bottleneck.** An autonomous OS build might generate millions of tokens over days. At ~$15-75/M tokens, that could be $500-$5,000+ for a single build run.
- Still dependent on someone else's infrastructure
- Latency remains network-bound

### Why Not Jump Straight to a Cloud GPU?

A vanilla open-source model (Llama 70B, DeepSeek-V3, etc.) hosted on a cloud GPU is just a worse Opus. The entire value proposition is **fine-tuning on Fawx's own signal data** — that's what closes the capability gap for this specific domain.

## Current State of Open-Source Models vs Opus

No open-source model running on local hardware matches Opus 4.6 today. The gap is closing but remains significant:

| Task | Best Open Model vs Opus |
|------|------------------------|
| Simple code generation | ~85-90% as good |
| Complex multi-file refactoring | ~60-70% |
| Architectural reasoning | ~50-60% |
| Long autonomous operation | ~40-50% |

The models that get closest (Llama 4 Maverick 400B, DeepSeek-V3 671B) require multiple high-end GPUs ($50K-$150K+ in hardware) to run locally. Quantization to fit consumer hardware degrades the capabilities that matter most: long reasoning chains, self-correction, and sustained coherence.

**Key insight:** A 70B+ model fine-tuned specifically for Fawx's tool-calling patterns, error recovery, and codebase conventions can **outperform a general-purpose Opus on Fawx-specific tasks** — even if it's weaker on general reasoning.

## The Pipeline

```
Signal data → SessionExporter → DPO pairs → LoRA fine-tune (Prime Intellect) → trained model → host on cloud GPU
```

### Phase 1: Data Accumulation (Happening Now)

The signal bus is already generating training data with every session. What's needed:

- **Build `SessionExporter`** — Converts raw signal data into structured DPO (Direct Preference Optimization) pairs
- **Build `TrainingManager`** — Manages dataset curation, versioning, and submission
- **Curation filter** — Only include pairs where:
  - The tool call succeeded
  - The verify step passed
  - No friction signal fired within the same cycle
- **Target:** 500+ high-quality curated pairs before first training run

> The hard part isn't the training — it's curation. Bad training data will degrade performance. The filter needs to be aggressive.

These components are specced in [#1004 LoRA Fine-Tuning Pipeline](./1004-lora-fine-tuning.md).

### Phase 2: Training (Prime Intellect)

Use Prime Intellect's decentralized GPU compute for training:

- **Submit:** Base model (Llama 4 Maverick 400B or best available at time of training) + curated DPO dataset
- **Receive:** LoRA adapter weights

**Why Prime Intellect over renting your own cluster:**

- Distributed training across commodity GPUs is cheaper than dedicated H100 rental
- Can train on larger models than a single rented node could handle
- No training infrastructure to manage — submit job, get back weights

### Phase 3: Inference (Cloud GPU)

Host the base model + LoRA adapter on cloud GPU infrastructure (RunPod, Lambda, Vast.ai):

- Load base model + trained LoRA adapter
- Fawx connects via the existing `CompletionProvider` trait in `fx-llm`
- **Cost:** ~$2-5/hr for inference

### Phase 4: Hybrid Routing (Steady State)

```
┌─────────────────────────────────┐
│        Fawx Decision Router     │
├────────────┬────────────────────┤
│ Routine    │ Complex            │
│ (80-90%)   │ (10-20%)           │
│            │                    │
│ Self-hosted│ Opus API           │
│ fine-tuned │ (pay per call)     │
│ 70B+ model │                    │
│            │                    │
│ ~$3/hr     │ Escalation only    │
└────────────┴────────────────────┘
```

- **Self-hosted model** handles the volume: file reads, build monitoring, routine error handling, command execution
- **Opus** handles the hard stuff: architectural decisions, novel error diagnosis, complex planning
- **Router logic:** Confidence threshold, task complexity classification, or "if the local model says it's unsure, escalate"

**Estimated cost for a multi-day autonomous operation (e.g., OS build):**
- ~$50-150 in GPU rental for the self-hosted model
- ~$50-200 in Opus escalation calls
- **Total: $100-350** vs $500-5,000+ for pure Opus

## Continuous Improvement Loop

Once this pipeline is running, **every session makes the trained model better:**

1. Fawx operates using the hybrid router
2. Signal bus captures all decisions, outcomes, friction, and escalations
3. `SessionExporter` converts new signals into DPO pairs
4. Periodically retrain with accumulated data on Prime Intellect
5. Deploy updated adapter weights to cloud GPU
6. The trained model handles a larger percentage of calls → fewer Opus escalations → lower cost

## Implementation Order

1. **Now:** Keep using Opus for daily work. Every session generates signal data.
2. **Build:** `SessionExporter` and `TrainingManager` (LoRA spec items 1-2)
3. **At 500+ curated pairs:** Submit first training run to Prime Intellect
4. **Validate:** Test adapter weights against held-out evaluation set
5. **Deploy:** Spin up cloud GPU, load base + adapter, point Fawx at it
6. **Route:** Implement hybrid routing in `fx-llm` — trained model for routine, Opus for escalation
7. **Iterate:** Continuous retraining as signal data accumulates

## Dependencies

- [#1004 LoRA Fine-Tuning Pipeline](./1004-lora-fine-tuning.md) — `SessionExporter`, `TrainingManager`, adapter loading
- [#1055 Self-Improvement Trigger](./1055-self-improvement-trigger.md) — Signal analysis pipeline that feeds training data
- `fx-llm` `CompletionProvider` trait — Already supports multiple backends; needs routing layer
- `fx-memory` `SignalStore` — Signal data accumulation (already operational)

## Open Questions

- What base model to target for first training run? (Depends on what's best when we hit 500 pairs)
- Prime Intellect pricing and job submission API — needs investigation
- Minimum viable router: simple confidence threshold vs learned classifier?
- How often to retrain? Per-session, weekly, on-demand?
