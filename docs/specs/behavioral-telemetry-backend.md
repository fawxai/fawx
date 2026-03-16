# Behavioral Telemetry Backend Spec

**Crate:** new `fx-telemetry` + endpoints in `fx-api`  
**Difficulty:** Medium  
**Status:** 0% — no backend, no types  

---

## Vision

Fawx reports **behavioral signals** — not user content — that help improve the kernel. Tool success/failure rates, proposal gate firing patterns, experiment scores, retry rates, error categories. The user controls exactly what gets shared via a granular consent framework. The data is anonymous, aggregated, and used exclusively for kernel improvement.

"Your agent gets smarter. Your data stays yours."

---

## Architecture

### Core Types

```rust
/// A single telemetry signal emitted by the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySignal {
    pub id: Uuid,
    pub category: SignalCategory,
    pub event: String,
    pub value: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    /// Session-scoped random ID (not persistent across restarts).
    pub session_id: String,
}

/// Categories of telemetry signals.
/// Each category can be independently enabled/disabled by the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SignalCategory {
    /// Tool execution success/failure rates.
    ToolUsage,
    /// Proposal gate firing (how often the agent hits safety gates).
    ProposalGate,
    /// Experiment outcomes (scores, decisions, timing).
    Experiments,
    /// Error rates and categories.
    Errors,
    /// Model/thinking usage patterns (which models, which thinking levels).
    ModelUsage,
    /// Performance metrics (response time, token counts).
    Performance,
}

impl SignalCategory {
    pub fn all() -> Vec<Self> {
        vec![
            Self::ToolUsage,
            Self::ProposalGate,
            Self::Experiments,
            Self::Errors,
            Self::ModelUsage,
            Self::Performance,
        ]
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::ToolUsage => "Which tools succeed/fail and how often",
            Self::ProposalGate => "How often the safety gate activates",
            Self::Experiments => "Experiment scores and outcomes (no code content)",
            Self::Errors => "Error rates and categories",
            Self::ModelUsage => "Which models and thinking levels are used",
            Self::Performance => "Response times and token counts",
        }
    }
}
```

### Consent Framework

```rust
/// User's telemetry consent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConsent {
    /// Master switch — if false, nothing is collected or sent.
    pub enabled: bool,
    /// Per-category consent. Only categories present AND true are collected.
    pub categories: HashMap<SignalCategory, bool>,
    /// When consent was last modified.
    pub updated_at: DateTime<Utc>,
}

impl Default for TelemetryConsent {
    fn default() -> Self {
        Self {
            enabled: false, // opt-IN, not opt-out
            categories: HashMap::new(),
            updated_at: Utc::now(),
        }
    }
}

impl TelemetryConsent {
    /// Check if a specific category is consented.
    pub fn is_category_enabled(&self, category: &SignalCategory) -> bool {
        self.enabled && self.categories.get(category).copied().unwrap_or(false)
    }

    /// Enable all categories.
    pub fn enable_all(&mut self) {
        self.enabled = true;
        for category in SignalCategory::all() {
            self.categories.insert(category, true);
        }
        self.updated_at = Utc::now();
    }

    /// Disable everything.
    pub fn disable_all(&mut self) {
        self.enabled = false;
        self.updated_at = Utc::now();
    }
}
```

### Signal Collector

```rust
/// Collects telemetry signals in memory, respecting consent.
pub struct SignalCollector {
    consent: RwLock<TelemetryConsent>,
    buffer: RwLock<Vec<TelemetrySignal>>,
    session_id: String,
    max_buffer_size: usize,
}

impl SignalCollector {
    pub fn new(consent: TelemetryConsent) -> Self;

    /// Record a signal. Silently dropped if category not consented.
    pub fn record(&self, category: SignalCategory, event: &str, value: serde_json::Value);

    /// Drain the buffer (for upload or export).
    pub fn drain(&self) -> Vec<TelemetrySignal>;

    /// Current buffer size.
    pub fn pending_count(&self) -> usize;

    /// Update consent. Drops buffered signals for newly-disabled categories.
    pub fn update_consent(&self, consent: TelemetryConsent);

    /// Get current consent state.
    pub fn consent(&self) -> TelemetryConsent;
}
```

### Signal Emitter Points

Signals are emitted from existing engine code. Each emitter calls `collector.record()`:

1. **ToolUsage** — in `ToolExecutor` after each tool call:
   - `{ tool: "read_file", success: true, duration_ms: 42 }`

2. **ProposalGate** — in `ProposalGateExecutor`:
   - `{ action: "file_write", decision: "approved" | "denied" | "prompted", path_tier: 1 }`

3. **Experiments** — in `ExperimentRunner` after each run:
   - `{ signal: "latency", decision: "accept", best_score: 0.85, candidates: 3, duration_secs: 120 }`

4. **Errors** — in `HeadlessApp::record_error`:
   - `{ category: "provider", message_hash: "sha256_first_8_chars", recoverable: true }`
   (message is hashed, not sent in cleartext)

5. **ModelUsage** — on model switch and thinking level change:
   - `{ model: "claude-sonnet-4-6", thinking: "high", provider: "anthropic" }`

6. **Performance** — after each completion:
   - `{ input_tokens: 1200, output_tokens: 450, duration_ms: 3200 }`

### HTTP API

```
GET  /v1/telemetry/consent     → current TelemetryConsent
PATCH /v1/telemetry/consent    → update consent (partial update)
GET  /v1/telemetry/signals     → buffered signals (for debugging/export)
POST /v1/telemetry/export      → export signals as JSON file
DELETE /v1/telemetry/signals   → clear the buffer
```

### Storage

- **Consent** stored in `config.toml` under `[telemetry]` section
- **Signals** buffered in memory only (Phase 1) — no disk persistence
- **Upload** endpoint is Phase 2 (needs backend server to receive signals)

---

## What We Explicitly Do NOT Collect

- No conversation content, messages, or prompts
- No file contents or paths (just tool names)
- No API keys, tokens, or credentials
- No IP addresses or device identifiers
- No error message text (only hashed + categorized)
- Session ID is random per restart (not persistent, not linkable)

---

## File Layout

```
fx-telemetry/
├── Cargo.toml
├── src/
│   ├── lib.rs          ← TelemetrySignal, SignalCategory, re-exports
│   ├── consent.rs      ← TelemetryConsent
│   ├── collector.rs    ← SignalCollector
│   └── error.rs        ← TelemetryError
```

Plus endpoints in `fx-api/src/handlers/telemetry.rs`.

---

## Implementation Phases

**Phase 1 (this PR):** Types + SignalCollector + consent API + export endpoint. No upload, no emitter wiring. ~500-800 lines.

**Phase 2:** Wire emitter points into engine code (ToolExecutor, ProposalGate, ExperimentRunner, HeadlessApp). Each is a 1-3 line addition.

**Phase 3:** Upload endpoint — encrypted POST to a Fawx telemetry server (if/when we build one).

---

## Test Plan (Phase 1)

1. **SignalCategory** — all() covers every variant, description non-empty
2. **TelemetryConsent** — default is disabled, enable_all enables everything, is_category_enabled respects master switch + per-category
3. **SignalCollector** — record respects consent, drain clears buffer, update_consent drops disabled signals, max_buffer_size enforced
4. **Serialization** — all types roundtrip through JSON
5. **Consent API** — GET returns current, PATCH updates specific categories

---

## Design Decisions

1. **Opt-in, not opt-out** — consent defaults to disabled. User must explicitly enable.
2. **Per-category granularity** — user can share tool usage but not model usage, etc.
3. **Memory-only buffer** — no disk write of telemetry signals (privacy-safe, restart clears)
4. **Hashed error messages** — we see error categories and frequency, not the actual error text
5. **Session-scoped random ID** — no way to link signals across restarts or to a user identity
6. **No phone-home in Phase 1** — signals are collected locally, exportable via API, but not uploaded anywhere
