# Behavioral Telemetry Pipeline Specification

**Status:** DRAFT  
**Phase:** Phase 5.5 / 6  
**Priority:** Lower — valuable but not blocking ship  
**Parent doc:** `docs/architecture/open-core-security-model.md`

---

## 1. Goal

Enable Fawx instances to contribute anonymous behavioral signals that guide kernel-level improvements, without exposing user content, requiring kernel source access, or creating a feedback loop from telemetry to the local instance.

---

## 2. Principles

1. **Signals, not content.** We collect outcome patterns (success/failure, timing, frequency) — never conversation content, file contents, or personal data.
2. **Off by default.** Telemetry requires explicit opt-in. First-run and setup wizard explain what's collected.
3. **Granular consent.** Users choose which signal categories to share. All-or-nothing is not the only option.
4. **Transparent.** Users can view exactly what signals are queued before they leave the device.
5. **One-way.** Signals flow from instance to aggregation service. The instance never receives aggregated results, telemetry-derived insights, or kernel update rationale.
6. **Differential privacy.** Individual usage patterns are not recoverable from aggregated data.

---

## 3. Signal Categories

### 3.1 Tool Execution Signals

| Signal | Fields | Content risk |
|--------|--------|-------------|
| Tool call outcome | `tool_name`, `success: bool`, `error_class: enum`, `latency_ms` | None — no arguments or content |
| Tool call frequency | `tool_name`, `calls_per_session: u32` | None |
| Tool retry pattern | `tool_name`, `retry_count`, `final_outcome` | None |

### 3.2 Proposal Gate Signals

| Signal | Fields | Content risk |
|--------|--------|-------------|
| Gate firing | `tier: enum`, `action_class: enum` (write/execute/install), `outcome: approved/denied` | None — no paths or content |
| Approval latency | `tier`, `seconds_to_decision` | None |
| Batch stats | `batch_size`, `approved_count`, `denied_count` | None |

### 3.3 Experiment Pipeline Signals

| Signal | Fields | Content risk |
|--------|--------|-------------|
| Experiment outcome | `scope_hash` (anonymized), `score`, `verdict: enum` | None — scope is hashed |
| Chain progression | `chain_length`, `score_trajectory: [f64]` | None |
| Build verification | `passed: bool`, `error_class: enum` | None |

### 3.4 Skill Lifecycle Signals

| Signal | Fields | Content risk |
|--------|--------|-------------|
| Skill install | `skill_id` (marketplace ID only), `tier: enum`, `source: marketplace/local/unsigned` | None |
| Skill usage | `skill_id`, `invocations_per_day: u32` | None |
| Skill uninstall | `skill_id`, `days_installed: u32` | None |

### 3.5 Session Signals

| Signal | Fields | Content risk |
|--------|--------|-------------|
| Session length | `turns: u32`, `duration_minutes: u32` | None |
| Compaction events | `compactions_per_session: u32`, `context_percentage_at_compaction: f32` | None |
| Model switch | `from_provider: enum`, `to_provider: enum` (no model names — just provider class) | Low — provider enum is coarse |

### Explicitly excluded

- Message content, prompts, or completions
- File names, paths, or contents
- User identifiers, device IDs, IP addresses
- Credential or auth information
- Memory/journal content
- Skill arguments or return values (only skill ID and invocation count)

---

## 4. Collection Architecture

### On-device

```
Tool Executor / Gate / Skills
    │
    ▼
Signal Collector (in-process, ring buffer)
    │
    ▼
Local Signal Store (~/.fawx/telemetry/pending/)
    │ (batched every 6 hours or on app close)
    ▼
Consent Gate (check enabled categories)
    │
    ▼
Inspection UI ("View pending signals")
    │
    ▼
Encrypted upload (TLS to telemetry endpoint)
```

### Ring buffer

Signals accumulate in a ring buffer (max 10,000 entries). Oldest entries are evicted when full. This bounds on-device storage and ensures the device doesn't accumulate unbounded telemetry if uploads fail.

### Batch upload

Signals are batched and uploaded every 6 hours (configurable) or when the app is closed. Failed uploads are retried on next batch cycle. After 7 days of failed uploads, stale signals are dropped.

### No device identifier

Each upload batch gets a random batch ID (UUID). There is no persistent device ID, user ID, or session ID in the telemetry payload. Batches from the same device are not linkable by design.

---

## 5. Server-Side Aggregation

### Differential privacy

Before any analysis, signals are aggregated with differential privacy:
- Laplace noise added to frequency counts
- k-anonymity threshold: signals with <10 contributing devices are suppressed
- No individual device can be identified from aggregated results

### Storage

- Raw signals are processed into aggregates and then deleted (max retention: 30 days for raw, indefinite for aggregates)
- Aggregates are stored by signal category, time period (daily/weekly), and anonymized dimensions
- No join keys exist between telemetry and any other data source

### Access

- Only the Fawx kernel team has access to aggregated results
- No automated pipeline from telemetry to kernel changes — humans analyze and decide
- Quarterly transparency report: published summary of what signals were collected, aggregate statistics, and any kernel changes informed by telemetry

---

## 6. Consent UX

### Setup wizard

During setup (Phase 4 wizard or `fawx setup`):
```
Fawx can share anonymous usage signals to help improve the engine.
What's shared: tool success rates, error patterns, skill popularity
What's NEVER shared: your conversations, files, or personal data

[View detailed signal list]

○ Share all signal categories
○ Choose which categories to share
○ Don't share anything

You can change this anytime in Settings.
```

### Settings panel

```
Telemetry
├── Enabled: [toggle]
├── Categories
│   ├── Tool execution patterns: [toggle]
│   ├── Proposal gate patterns: [toggle]  
│   ├── Experiment outcomes: [toggle]
│   ├── Skill lifecycle: [toggle]
│   └── Session statistics: [toggle]
├── View pending signals (X signals queued)
└── Last upload: [timestamp] (Y signals sent)
```

### "View pending signals" screen

Shows the actual JSON payloads queued for upload. Users can:
- Read every signal before it leaves the device
- Delete individual signals
- Clear all pending signals

---

## 7. Testing

### Unit tests
- Each signal type is correctly captured with expected fields
- Ring buffer evicts oldest entries at capacity
- Consent gate filters signals by enabled categories
- Disabled categories produce zero signals (not collected, not just filtered)
- No user content appears in any signal payload (content-free assertion)

### Integration tests
- End-to-end: tool call → signal captured → batch formed → upload succeeds
- Consent flow: enable/disable categories → verify signal inclusion/exclusion
- Inspection UI: pending signals match actual payloads
- Upload failure: signals retained and retried next cycle
- 7-day staleness: old signals dropped after max retry window

### Privacy tests
- Batch payloads contain no device ID, user ID, IP, or session ID
- Two batches from the same device are not linkable
- Signal payloads contain no file paths, message content, or arguments
- Differential privacy: aggregated results suppress low-count signals

---

## 8. Acceptance Criteria

1. Telemetry is off by default — requires explicit opt-in
2. Users can view, delete, and control signals before they leave the device
3. No user content (messages, files, paths, arguments) appears in any signal
4. No persistent device identifier exists in telemetry payloads
5. Consent is granular per signal category
6. Server-side aggregation uses differential privacy
7. Raw signals deleted within 30 days of processing
8. Quarterly transparency report published
