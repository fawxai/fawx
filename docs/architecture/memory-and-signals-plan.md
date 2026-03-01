# Memory & Signals Plan

**Status:** Implementing  
**Date:** 2026-03-01  
**Related:** `memory-systems.html`, `memory-augmented-intelligence.html`, `memory-dreaming.html`  
**Epic:** #998  

## Overview

This document describes the layered plan for evolving Fawx's memory and signal
systems from flat key-value storage toward metadata-rich, consolidation-aware
memory that feeds into anticipatory signal analysis.

## Layer 5: Memory System — Metadata

### MemoryEntry Schema

Each memory entry carries lightweight metadata alongside its value:

```rust
pub struct MemoryEntry {
    pub value: String,
    pub created_at_ms: u64,
    pub last_accessed_at_ms: u64,
    pub access_count: u32,
    pub source: MemorySource,
    pub tags: Vec<String>,
}
```

| Field | Purpose |
|-------|--------|
| `value` | The stored content (unchanged from flat format) |
| `created_at_ms` | When the entry was first written |
| `last_accessed_at_ms` | Last time the entry was read via tool |
| `access_count` | Number of tool reads (saturating) |
| `source` | Origin: `User`, `SignalAnalysis`, or `Consolidation` |
| `tags` | Free-form labels for emergent categorization |

### MemorySource Enum

```rust
pub enum MemorySource {
    User,            // Written via memory_write tool
    SignalAnalysis,  // Produced by signal analysis pipeline
    Consolidation,   // Created during memory dreaming/consolidation
}
```

Source is typed (not stringly-typed) to prevent invalid origins and enable
pattern matching in downstream consumers.

### Backward Compatibility

- Old `HashMap<String, String>` files auto-migrate on load
- Migrated entries get `source: User` and `created_at_ms` from file mtime
- New fields use `#[serde(default)]` for forward compatibility with
  future schema additions

### Access Tracking

`touch()` is separated from `read()` at the trait level because `read()` is
non-mutating (`&self`) while `touch()` requires `&mut self`. The canonical
pattern for read-with-tracking is:

```rust
let value = store.read(key);
if value.is_some() {
    store.touch(key)?;
}
```

The `MemoryStore` trait combines both capabilities for callers that need both.

### Snapshot Sort Contract

The `snapshot()` method returns entries ordered by descending `access_count`,
with ties broken by ascending key name. This puts the most-accessed memories
first in the system prompt context window.

## Future Layers

### Layer 6: Signal Analysis Integration

Memories created by signal analysis will use `MemorySource::SignalAnalysis`,
enabling the system to distinguish user-supplied facts from inferred knowledge.

### Layer 7: Memory Consolidation ("Dreaming")

The consolidation pipeline will merge, prune, and synthesize memories during
idle periods. Consolidated entries use `MemorySource::Consolidation` and
preserve provenance through tags.

See `memory-dreaming.html` for the full consolidation design.
