# RFC: Agent Error Visibility & Inner Monologue Policy

**Issue:** #480  
**Author:** Jarvis  
**Status:** APPROVED → ALL PHASES COMPLETE
**Date:** 2026-02-17

## Problem

When the agent runs a tool loop, intermediate failures are a normal part of exploration — tap on wrong element, app not found, screen content unexpected. These are the agent's problem to solve, not noise for the user. But some errors (accessibility lost, API down) ARE things the user needs to know about.

**Current state:** `OutputClassifier.classify()` treats ALL errors identically — `isError = true` → `OutputVisibility.SHOW`. Every failed grep, missed tap, and network blip gets surfaced as a prominent error message in the chat. This worked when the loop was simple, but with 25+ step loops it becomes unreadable.

There's also no framework for the agent's inner monologue — how it decides what to communicate from its reasoning process.

## Design

### 1. Error Severity Taxonomy

New enum in `OutputClassifier.kt`:

```kotlin
enum class ErrorSeverity {
    EXPLORATORY,
    TRANSIENT,
    PERSISTENT,
    INFORMATIONAL
}
```

### 2. Error Classification Logic

New method in `OutputClassifier`:

```kotlin
fun classifyError(
    toolName: String,
    errorText: String,
    retryContext: RetryContext? = null
): ErrorSeverity
```

Classification rules (evaluated in order):

| Signal | Severity | Rationale |
|--------|----------|-----------|
| Accessibility lost + reconnect failed | PERSISTENT | User must act |
| API auth failure (401/403) | PERSISTENT | Key is broken |
| API rate limit (429) after 3 retries | PERSISTENT | Can't continue |
| API server error (5xx), first occurrence | TRANSIENT | Usually recovers |
| `"element not found"` / `"could not tap"` | EXPLORATORY | Agent explores differently |
| `"no results"` from search/fetch | INFORMATIONAL | User should know search failed |
| `"app not installed"` / `"permission denied"` | INFORMATIONAL | User might need to act |
| Network timeout, first occurrence | TRANSIENT | Retry likely works |
| Network timeout, 3rd+ occurrence | PERSISTENT | Connectivity issue |
| Unknown error on MECHANICAL tool | EXPLORATORY | Default: agent handles it |
| Unknown error on PROMINENT/RESEARCH tool | INFORMATIONAL | Higher-value failures matter |

**RetryContext** (optional, for escalation):

```kotlin
data class RetryContext(
    val consecutiveFailures: Int = 0,
    val escalateToTransientAt: Int = 2,
    val escalateToPersistentAt: Int = 3
)
```

### 3. Visibility Mapping

| Severity | OutputVisibility | Status Bar | Chat Message | Audio |
|----------|-----------------|------------|--------------|-------|
| EXPLORATORY | HIDE | No change | Nothing | Silent |
| TRANSIENT | HIDE | Brief flash: "Retrying..." | Nothing (unless escalates) | Silent |
| PERSISTENT | SHOW | "⚠️ {description}" (stays) | Red error message | Announce |
| INFORMATIONAL | SHOW_DIMMED | No change | Dimmed message | Optional |

### 4. Integration Points

#### 4.1 AgentExecutor Changes

Classification happens in `OutputClassifier.classify()` which receives `isError`.

#### 4.2 LoopProgressListener Extension (Phase 2)

New callback for error-specific UI updates.

#### 4.3 ChatViewModel Status Updates (Phase 2)

Status bar integration for TRANSIENT/PERSISTENT errors.

#### 4.4 RetryContext Tracking (Phase 2)

Failure counting in AgentExecutor.

### 5. Inner Monologue Policy

Prompt-driven communication guidelines for the agent.

### 6. Verbosity Interaction

Error severity interacts with `OutputVerbosity`:

| Severity | MINIMAL | NORMAL | VERBOSE |
|----------|---------|--------|---------|
| EXPLORATORY | HIDE | HIDE | SHOW_DIMMED |
| TRANSIENT | HIDE | HIDE | SHOW_DIMMED |
| PERSISTENT | SHOW | SHOW | SHOW |
| INFORMATIONAL | HIDE | SHOW_DIMMED | SHOW |

### 7. Implementation Plan

**Phase 1 — Core classification (1 PR):**
- `ErrorSeverity` enum
- `classifyError()` in `OutputClassifier`
- Update `classify()` to use error severity
- `RetryContext` data class
- Tests for all classification rules

**Phase 2 — Loop integration (1 PR):**
- `onToolError()` in `LoopProgressListener`
- Failure counting in `AgentExecutor`
- `ChatViewModel` status bar error handling
- Tests for escalation and status updates

**Phase 3 — Prompt tuning (1 PR):**
- Communication policy in system/action prompt
- Validate with real tool loops

## Decisions (resolved 2026-02-17)

1. **Retry counter:** No counter — just "Retrying..." is sufficient.
2. **Severity ownership:** `ErrorSeverity` lives on `ToolResult` directly with classifier fallback.
3. **Escalation thresholds:** 2 (→TRANSIENT) and 3 (→PERSISTENT) confirmed as defaults.

---

**Status:** APPROVED — ALL PHASES COMPLETE
