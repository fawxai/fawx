# Spec: Tool Result Caching and Deduplication (#1054)

**Status:** Draft  
**Author:** Scoping agent  
**Date:** 2026-03-02  
**Complexity:** Medium (~320–420 lines of new code + ~220 lines of tests)

---

## 1. Problem Statement

During a single agentic loop cycle, the LLM frequently re-invokes identical tools with identical arguments across iterations and sub-goal executions. Common patterns:

- **Repeated `read_file`**: The LLM reads `Cargo.toml` or `README.md` multiple times across tool-continuation rounds within the same cycle.
- **Repeated `list_directory`**: The same directory listing is requested across sub-goals or iterations.
- **Repeated `search_text`**: Identical grep/search queries re-execute against unchanged files.

Each redundant call:
1. **Burns tokens** — the tool output gets serialized into continuation messages, consuming input tokens on every subsequent LLM call in the round.
2. **Adds latency** — file I/O and especially `run_command` have non-trivial wall-clock cost.
3. **Bloats context** — duplicate results fill the context window, potentially triggering compaction earlier.

**Scope:** This spec covers intra-cycle caching only. Cross-cycle persistence is explicitly out of scope (each `run_cycle` starts fresh).

---

## 2. Exact Files to Change

### New file

| File | Purpose |
|------|---------|
| `engine/crates/fx-kernel/src/caching_executor.rs` | `CachingExecutor<T>` wrapper, `CacheKey`, cache storage, invalidation |

### Modified files

| File | Lines (approx) | Change |
|------|----------------|--------|
| `engine/crates/fx-kernel/src/lib.rs` | module declarations | Add `pub mod caching_executor;` |
| `engine/crates/fx-kernel/src/act.rs` | `ToolExecutor` trait (~L92-118) | Add `ToolCacheability`; add `cacheability()` defaulting to `NeverCache`; add `clear_cache()` default no-op; add `cache_stats()` defaulting to `None` |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `prepare_cycle` + cycle-finalization path | Call `self.tool_executor.clear_cache()` at cycle boundary and emit cache performance signal via `self.tool_executor.cache_stats()` |
| `engine/crates/fx-cli/src/tui.rs` | `build_skill_registry` (~L1930-1960) | Wrap registry with `CachingExecutor::new(registry)` before `LoopEngine::new` |
| `engine/crates/fx-loadable/src/skill.rs` | `Skill` trait | Add skill-level `cacheability()` defaulting to `NeverCache` |
| `engine/crates/fx-loadable/src/registry.rs` | `dispatch_call` + `ToolExecutor` impl (currently ~L82-114 and ~L236-256) | Implement `ToolExecutor::cacheability()` by delegating to the owning skill |
| `engine/crates/fx-tools/src/skill_bridge.rs` | `BuiltinToolsSkill` | Delegate `Skill::cacheability()` to `FawxToolExecutor::cacheability()` |
| `engine/crates/fx-tools/src/tools.rs` | `FawxToolExecutor` impl | Implement built-in tool classification (`Cacheable` / `NeverCache` / `SideEffect`) |

---

## 3. API Design

### 3.1 `ToolCacheability` enum

```rust
// In fx-kernel/src/act.rs

/// Declares whether a tool's results can be cached for identical inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCacheability {
    /// Results are deterministic for identical arguments within a cycle.
    Cacheable,
    /// Results should never be cached.
    NeverCache,
    /// Tool has side effects and may invalidate cache entries.
    /// Result itself is never cached.
    SideEffect,
}
```

### 3.2 `ToolExecutor` trait additions (opt-in caching)

```rust
// In fx-kernel/src/act.rs

/// Cache counters exposed by caching-capable executors.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: u64,
    pub evictions: u64,
}

/// Classify whether a tool's results are cacheable.
/// Default is conservative: tools are NOT cached unless explicitly opted in.
fn cacheability(&self, tool_name: &str) -> ToolCacheability {
    let _ = tool_name;
    ToolCacheability::NeverCache
}

/// Clear cached tool results at cycle boundaries.
/// For caching executors, this is the cycle reset hook.
/// `CachingExecutor` clears entries/indexes and resets hit/miss/eviction counters
/// so `cache_stats()` reflects the current cycle only.
/// Default no-op for non-caching executors.
fn clear_cache(&self) {}

/// Return cache stats when supported by this executor.
/// Default `None` keeps non-caching executors trait-object compatible.
fn cache_stats(&self) -> Option<ToolCacheStats> {
    None
}
```

This flips the previous default and ensures new tools/skills are safe by default while giving `LoopEngine` a trait-level stats access path through `Arc<dyn ToolExecutor>`.

### 3.3 `CacheKey` + `normalize_json()`

```rust
// In fx-kernel/src/caching_executor.rs

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    tool_name: String,
    args_hash: u64,
}

impl CacheKey {
    fn new(tool_name: &str, arguments: &serde_json::Value) -> Self {
        use std::hash::{Hash, Hasher};

        // V1: std DefaultHasher (no new dependency per ENGINEERING.md).
        // Future V2 candidate: ahash/foldhash.
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        normalize_json(arguments).hash(&mut hasher);

        Self {
            tool_name: tool_name.to_string(),
            args_hash: hasher.finish(),
        }
    }
}

fn normalize_json(value: &serde_json::Value) -> String {
    fn normalize_value(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().cloned().collect();
                keys.sort();
                let mut normalized = serde_json::Map::with_capacity(map.len());
                for key in keys {
                    let child = map.get(&key).expect("key exists");
                    normalized.insert(key, normalize_value(child));
                }
                serde_json::Value::Object(normalized)
            }
            serde_json::Value::Array(items) => serde_json::Value::Array(
                items.iter().map(normalize_value).collect(),
            ),
            _ => value.clone(),
        }
    }

    normalize_value(value).to_string()
}
```

Properties:
- Objects are recursively key-sorted.
- Arrays preserve original order.
- Nested objects are sorted recursively.
- Final canonical string is used for hashing.
- `args_hash` is `u64`; theoretical collision risk is acknowledged in §6 Risks.

### 3.4 `CachingExecutor` wrapper and cache storage

```rust
use crate::act::{
    ConcurrencyPolicy, ToolCacheability, ToolCacheStats, ToolExecutor, ToolExecutorError,
    ToolResult,
};
use crate::cancellation::CancellationToken;
use async_trait::async_trait;
use fx_llm::{ToolCall, ToolDefinition};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use tracing::warn;

const MAX_CACHE_ENTRIES: usize = 256;

#[derive(Debug)]
pub struct CachingExecutor<T: ToolExecutor> {
    inner: T,
    cache: Mutex<ToolCache>,
}

#[derive(Debug, Default)]
struct ToolCache {
    entries: HashMap<CacheKey, CachedResult>,
    /// FIFO insertion order for oldest-eviction.
    order: VecDeque<CacheKey>,
    /// Reverse index: path -> all cache keys that depended on this path.
    path_index: HashMap<String, HashSet<CacheKey>>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

#[derive(Debug, Clone)]
struct CachedResult {
    output: String,
    success: bool,
    indexed_paths: Vec<String>,
}

impl<T: ToolExecutor> CachingExecutor<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            cache: Mutex::new(ToolCache::default()),
        }
    }

    fn reset_cache_state(&self) {
        match self.cache.lock() {
            Ok(mut cache) => {
                cache.entries.clear();
                cache.order.clear();
                cache.path_index.clear();
                cache.hits = 0;
                cache.misses = 0;
                cache.evictions = 0;
            }
            Err(_) => {
                // Intentional graceful degradation: cache is an optimization;
                // execution should continue even if cache state is poisoned.
                warn!("tool cache lock poisoned during cache reset; skipping cache reset");
            }
        }
    }

    fn lookup(&self, key: &CacheKey) -> Option<CachedResult> {
        let mut cache = match self.cache.lock() {
            Ok(cache) => cache,
            Err(_) => {
                // Intentional graceful degradation to uncached behavior on poison.
                warn!("tool cache lock poisoned during lookup; treating as cache miss");
                return None;
            }
        };
        let hit = cache.entries.get(key).cloned();

        if hit.is_some() {
            cache.hits += 1;
        } else {
            cache.misses += 1;
        }

        hit
    }

    fn cache_stats_snapshot(&self) -> Option<ToolCacheStats> {
        let cache = match self.cache.lock() {
            Ok(cache) => cache,
            Err(_) => {
                warn!("tool cache lock poisoned while reading cache stats; skipping stats emission");
                return None;
            }
        };

        Some(ToolCacheStats {
            hits: cache.hits,
            misses: cache.misses,
            entries: cache.entries.len() as u64,
            evictions: cache.evictions,
        })
    }
}
```

`std::sync::Mutex` is intentional:
- We never hold the lock across `.await`, so an async mutex is unnecessary.
- Use `std::sync::Mutex<ToolCache>` (not `tokio::sync::Mutex`) to avoid async lock overhead.
- `lookup`, `store`, and invalidation take short critical sections only.
- On poisoned locks, emit a warning and degrade to uncached behavior rather than failing tool execution (cache is optimization-only).
- Inherent helper naming uses `reset_cache_state()` so the trait hook `clear_cache()` remains unambiguous at call sites.

### 3.5 `ToolExecutor` implementation with concrete result ordering algorithm

```rust
#[async_trait]
impl<T: ToolExecutor> ToolExecutor for CachingExecutor<T> {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        // Phase 0: pre-allocate final result slots in original order.
        let mut ordered_results: Vec<Option<ToolResult>> = vec![None; calls.len()];
        let mut uncached_calls = Vec::new();
        let mut uncached_indices = Vec::new();

        // Phase 1: resolve cache hits at original indices.
        for (index, call) in calls.iter().enumerate() {
            match self.inner.cacheability(&call.name) {
                ToolCacheability::Cacheable => {
                    let key = CacheKey::new(&call.name, &call.arguments);
                    if let Some(cached) = self.lookup(&key) {
                        ordered_results[index] = Some(ToolResult {
                            tool_call_id: call.id.clone(),
                            tool_name: call.name.clone(),
                            success: cached.success,
                            output: cached.output,
                        });
                    } else {
                        uncached_calls.push(call.clone());
                        uncached_indices.push(index);
                    }
                }
                ToolCacheability::NeverCache | ToolCacheability::SideEffect => {
                    uncached_calls.push(call.clone());
                    uncached_indices.push(index);
                }
            }
        }

        // Phase 2: execute misses / uncached calls.
        if !uncached_calls.is_empty() {
            let executed_results = self.inner.execute_tools(&uncached_calls, cancel).await?;

            // Phase 3: store or invalidate, then place each result back to original index.
            for ((original_index, call), result) in uncached_indices
                .iter()
                .copied()
                .zip(uncached_calls.iter())
                .zip(executed_results.into_iter())
            {
                match self.inner.cacheability(&call.name) {
                    ToolCacheability::Cacheable if result.success => {
                        self.store(
                            CacheKey::new(&call.name, &call.arguments),
                            &call.name,
                            &call.arguments,
                            &result,
                        );
                    }
                    ToolCacheability::SideEffect => {
                        self.invalidate_for_side_effect(&call.name, &call.arguments);
                    }
                    ToolCacheability::NeverCache | ToolCacheability::Cacheable => {}
                }

                ordered_results[original_index] = Some(result);
            }
        }

        // Phase 4: convert in-order slots into final output with explicit error reporting.
        let results = ordered_results
            .into_iter()
            .enumerate()
            .map(|(index, maybe_result)| {
                maybe_result.ok_or_else(|| ToolExecutorError {
                    message: format!(
                        "caching executor missing tool result at slot {index}"
                    ),
                    recoverable: false,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.inner.tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.inner.cacheability(tool_name)
    }

    fn clear_cache(&self) {
        self.reset_cache_state();
    }

    fn cache_stats(&self) -> Option<ToolCacheStats> {
        self.cache_stats_snapshot()
    }

    fn concurrency_policy(&self) -> ConcurrencyPolicy {
        self.inner.concurrency_policy()
    }
}
```

This removes the handwave and guarantees output order equals input order.

### 3.6 Invalidation strategy with wired `path_index`

Invalidation remains targeted, but now actually uses `path_index` and does not reconstruct synthetic keys.

```rust
fn extract_index_paths(tool_name: &str, arguments: &serde_json::Value) -> Vec<String> {
    match tool_name {
        "read_file" | "list_directory" | "search_text" => arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| vec![p.to_string()])
            .unwrap_or_default(),
        "memory_read" => arguments
            .get("key")
            .and_then(|v| v.as_str())
            .map(|k| vec![format!("memory:{k}")])
            .unwrap_or_default(),
        "memory_list" => vec!["memory:*".to_string()],
        _ => Vec::new(),
    }
}

fn store(
    &self,
    key: CacheKey,
    tool_name: &str,
    arguments: &serde_json::Value,
    result: &ToolResult,
) {
    let indexed_paths = extract_index_paths(tool_name, arguments);
    let mut cache = match self.cache.lock() {
        Ok(cache) => cache,
        Err(_) => {
            warn!("tool cache lock poisoned during store; skipping cache write");
            return;
        }
    };

    if cache.entries.len() >= MAX_CACHE_ENTRIES {
        cache.evict_oldest();
    }

    for path in &indexed_paths {
        cache
            .path_index
            .entry(path.clone())
            .or_default()
            .insert(key.clone());
    }

    cache.order.push_back(key.clone());
    cache.entries.insert(
        key,
        CachedResult {
            output: result.output.clone(),
            success: result.success,
            indexed_paths,
        },
    );
}

fn invalidate_for_side_effect(&self, tool_name: &str, arguments: &serde_json::Value) {
    let mut cache = match self.cache.lock() {
        Ok(cache) => cache,
        Err(_) => {
            warn!("tool cache lock poisoned during invalidation; skipping invalidation");
            return;
        }
    };

    match tool_name {
        "write_file" => {
            if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                cache.invalidate_path(path); // read_file(path), search_text(path), etc.
                if let Some(parent) = std::path::Path::new(path).parent() {
                    cache.invalidate_path(&parent.to_string_lossy()); // list_directory(parent)
                }
                cache.invalidate_tool("search_text"); // conservative fallback
            }
        }
        "memory_write" | "memory_delete" => {
            if let Some(key) = arguments.get("key").and_then(|v| v.as_str()) {
                cache.invalidate_path(&format!("memory:{key}"));
            }
            cache.invalidate_path("memory:*");
        }
        "run_command" => {
            // Conservative safe default: flush all cacheable entries.
            // V2 can optimize with command-aware invalidation.
            cache.flush_all_cacheable();
        }
        _ => {}
    }
}
```

Helper behavior in `ToolCache`:
- `invalidate_path(path)` reads all `CacheKey`s from `path_index[path]` and removes each key.
- `invalidate_tool(tool_name)` removes all entries in `entries` whose `CacheKey.tool_name` matches the given name, updating `order` and `path_index` via `remove_key()`.
- `remove_key()` removes from `entries`, `order`, and every path bucket in `path_index` using `CachedResult.indexed_paths`.
- `evict_oldest()` pops from `order` until it removes a still-live key, then updates reverse index and `evictions += 1`.
- `flush_all_cacheable()` removes all entries from the cache (entries/order/path_index); hit/miss/eviction counters are preserved within the current cycle and reset by `clear_cache()` at the next cycle boundary.

### 3.7 Built-in tool classification (`FawxToolExecutor`)

```rust
fn cacheability(&self, tool_name: &str) -> ToolCacheability {
    match tool_name {
        // Pure reads.
        "read_file" | "list_directory" | "search_text"
        | "memory_read" | "memory_list" => ToolCacheability::Cacheable,

        // Side effects + invalidation.
        "write_file" | "memory_write" | "memory_delete" | "run_command" => {
            ToolCacheability::SideEffect
        }

        // Time/external state and mutable runtime introspection.
        "current_time" | "self_info" => ToolCacheability::NeverCache,

        // Unknown tools must opt in explicitly.
        _ => ToolCacheability::NeverCache,
    }
}
```

`self_info` now uses `NeverCache` for strict correctness: in the current codebase (`fx-tools/src/tools.rs`) it reads `runtime_info` behind `Arc<RwLock<_>>`, and that state can change while a cycle is in progress.

### 3.8 `SkillRegistry` delegation model (correct references + routing)

The previous reference to `registry.rs ~L160-180` was wrong for the `ToolExecutor` impl.

Current structure:
- `dispatch_call` logic is around **`registry.rs` L82-114**.
- `impl ToolExecutor for SkillRegistry` is around **`registry.rs` L236-256**.

Design update:
1. Add `cacheability()` to `Skill` trait (`skill.rs`) defaulting to `NeverCache`.
2. In `SkillRegistry::cacheability(tool_name)`, locate the owning skill by tool definition.
3. Delegate to that skill’s `cacheability(tool_name)`.
4. If no skill owns tool, return `NeverCache`.

```rust
// fx-loadable/src/skill.rs
fn cacheability(&self, tool_name: &str) -> ToolCacheability {
    let _ = tool_name;
    ToolCacheability::NeverCache
}

// fx-loadable/src/registry.rs
impl SkillRegistry {
    fn owning_skill(&self, tool_name: &str) -> Option<&dyn Skill> {
        self.skills
            .iter()
            .find(|skill| {
                skill
                    .tool_definitions()
                    .iter()
                    .any(|definition| definition.name == tool_name)
            })
            .map(|skill| skill.as_ref())
    }
}

#[async_trait]
impl ToolExecutor for SkillRegistry {
    // ... execute_tools/tool_definitions unchanged ...

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.owning_skill(tool_name)
            .map(|skill| skill.cacheability(tool_name))
            .unwrap_or(ToolCacheability::NeverCache)
    }
}
```

`BuiltinToolsSkill` then forwards to `FawxToolExecutor`:

```rust
fn cacheability(&self, tool_name: &str) -> ToolCacheability {
    self.executor.cacheability(tool_name)
}
```

This makes cacheability ownership explicit and keeps the registry as a dispatcher, not a static classifier.

Performance note: `owning_skill()` is `O(skills × tools_per_skill)` because it scans definitions. That is acceptable for V1's small skill set; if tool count grows, add a `tool_owner_index` map built at registration time for O(1) lookup.

### 3.9 Cycle boundary + outermost-wrapper invariant

`prepare_cycle()` must clear cache:

```rust
fn prepare_cycle(&mut self) {
    self.iteration_count = 0;
    self.budget.reset(current_time_ms());
    self.signals.clear();
    self.user_stop_requested = false;
    if let Some(token) = &self.cancel_token {
        token.reset();
    }
    self.tool_executor.clear_cache();
}
```

**Invariant:** `CachingExecutor` must be the **outermost** `ToolExecutor` wrapper passed to `LoopEngine` so it observes all calls and all side effects in one place.

### 3.10 Performance signal shape for cache stats

`LoopEngine` retrieves stats through the trait object (`Arc<dyn ToolExecutor>`) — no downcast to `CachingExecutor` required.

Stats semantics are **per-cycle**: `prepare_cycle()` calls `clear_cache()`, and `CachingExecutor::clear_cache()` resets entries/indexes plus `hits`/`misses`/`evictions` to zero before each cycle begins.

```rust
fn finalize_result(&mut self, result: LoopResult) -> LoopResult {
    if let Some(stats) = self.tool_executor.cache_stats() {
        let total = stats.hits + stats.misses;
        let hit_rate = if total == 0 {
            0.0
        } else {
            stats.hits as f64 / total as f64
        };

        self.emit_signal(
            LoopStep::Act,
            SignalKind::Performance,
            "tool cache stats",
            serde_json::json!({
                "hits": stats.hits,
                "misses": stats.misses,
                "entries": stats.entries,
                "evictions": stats.evictions,
                "hit_rate": hit_rate,
            }),
        );
    }

    let signals = self.signals.drain_all();
    attach_signals(result, signals)
}
```

---

## 4. Implementation Plan

### Step 1: Add cacheability APIs (kernel + loadable)

- Add `ToolCacheability` to `fx-kernel/src/act.rs`
- Add `ToolExecutor::cacheability()` default `NeverCache`
- Add `ToolExecutor::cache_stats()` default `None` + `ToolCacheStats` struct
- Add `Skill::cacheability()` default `NeverCache`
- Keep `clear_cache()` on `ToolExecutor` as default no-op

### Step 2: Implement `CachingExecutor`

- Create `caching_executor.rs`
- Add `normalize_json()` canonicalization
- Add `CacheKey`, `ToolCache`, `CachedResult`
- Use `Mutex<ToolCache>`
- Add `MAX_CACHE_ENTRIES = 256` with oldest-eviction
- Implement full ordered-result algorithm with `Vec<Option<ToolResult>>`
- Use an internal helper like `reset_cache_state()` to avoid inherent/trait `clear_cache()` name collision

### Step 3: Wire path-indexed storage + invalidation

- `store()` adds key to `path_index`
- `invalidate_for_side_effect()` resolves keys via `path_index`
- `run_command` flushes all cacheable entries (conservative V1)

### Step 4: Add classifications + skill delegation

- `FawxToolExecutor::cacheability()` explicit classification
- `BuiltinToolsSkill::cacheability()` delegates to executor
- `SkillRegistry::cacheability()` routes to owning skill

### Step 5: Integration wiring

- Export module in `fx-kernel/src/lib.rs`
- Wrap registry with `CachingExecutor::new(registry)` in `fx-cli/src/tui.rs`
- Ensure this wrapper is outermost when passed into `LoopEngine`

### Step 6: Cycle boundary clear + observability

- `loop_engine.prepare_cycle()` calls `tool_executor.clear_cache()` (entry/index wipe + stats counter reset)
- `loop_engine.finalize_result()` reads `tool_executor.cache_stats()` and emits `SignalKind::Performance` when available

---

## 5. Test Plan

### Unit tests (`caching_executor.rs`)

| Test | Description |
|------|-------------|
| `cache_hit_returns_stored_result` | First `read_file(a)` miss, second identical call hit |
| `cache_miss_for_different_args` | `read_file(a)` vs `read_file(b)` both execute |
| `cache_miss_for_different_tools` | `read_file(a)` vs `list_directory(a)` both execute |
| `never_cache_tool_always_executes` | `current_time` executes each time |
| `side_effect_tool_not_cached` | `write_file` result never cached |
| `write_file_invalidates_read_file_via_path_index` | invalidation uses reverse index bucket for exact path |
| `write_file_invalidates_parent_list_directory_via_path_index` | parent path bucket invalidated |
| `memory_write_invalidates_memory_read_and_list` | `memory:key` + `memory:*` buckets invalidated |
| `run_command_flushes_cacheable_entries` | any prior cacheable entries removed |
| `clear_cache_resets_entries_indexes_and_stats` | clear removes entries/order/path_index and zeroes hits/misses/evictions |
| `cache_stats_tracks_hits_misses_evictions` | counters update correctly |
| `json_normalization_matches_reordered_keys` | object key order does not change key hash |
| `failed_tool_result_not_cached` | unsuccessful call is not stored |
| `oldest_entry_evicted_when_capacity_exceeded` | `MAX_CACHE_ENTRIES + 1` inserts evict oldest |
| `ordered_results_preserve_call_order_with_mixed_hits_and_misses` | validates `Vec<Option<_>>` ordering algorithm |
| `mixed_cacheability_batch_preserves_order_and_semantics` | one batch contains `Cacheable`, `NeverCache`, and `SideEffect` calls; output order and cache behavior remain correct |
| `tool_definitions_delegated` | pass-through behavior preserved |
| `concurrency_policy_delegated` | pass-through behavior preserved |

### Integration tests

| Test | Description |
|------|-------------|
| `cycle_boundary_clears_cache` | second cycle starts with empty cache |
| `sub_goal_shares_parent_cache` | shared `Arc<dyn ToolExecutor>` shares one cache |
| `sequential_execute_calls_share_cache` | two separate `execute_tools` calls run sequentially: first warms cache, second hits cache |
| `caching_executor_outermost_invariant` | wiring test ensures executor fed to loop is `CachingExecutor` outermost |
| `skill_registry_cacheability_delegates_to_owner` | registry routes tool name to owning skill cacheability |
| `loop_engine_emits_cache_stats_via_trait_object` | `Arc<dyn ToolExecutor>` path emits cache signal when `cache_stats()` is `Some` |

---

## 6. Invariants and Risks

### Invariants

1. **Semantic transparency:** same observable `ToolResult` outputs as no-cache execution (assuming no external mid-cycle mutation).
2. **Ordering correctness:** `execute_tools` output order matches input order exactly.
3. **Safety default:** tools are `NeverCache` unless they explicitly opt in.
4. **Wrapper topology:** `CachingExecutor` is outermost.

### Risks and mitigations

- **`run_command` can mutate anything** → V1 flushes all cacheable entries after every `run_command`.
- **Path aliasing (`./x` vs `x`)** → V1 indexes raw argument path strings; path canonicalization remains V2.
- **Memory growth** → bounded by `MAX_CACHE_ENTRIES = 256` + oldest eviction.
- **Hasher speed** → `DefaultHasher` in V1 for zero new deps; revisit `ahash`/`foldhash` in V2.
- **`args_hash` (`u64`) collisions** → theoretical correctness risk (a collision could return the wrong cached result). At `MAX_CACHE_ENTRIES = 256`, birthday-collision probability per cycle is ~10^-15; acceptable for V1. V2 can store full canonical args (or widen hash) if stronger guarantees are needed.

---

## 7. Estimated Complexity

| Component | Lines | Difficulty |
|-----------|-------|------------|
| `ToolCacheability` + trait methods | ~40 | Trivial |
| `CachingExecutor` core + ordering + invalidation | ~220 | Medium |
| Path index + eviction helpers | ~90 | Medium |
| Skill delegation (`Skill`, `SkillRegistry`, bridge) | ~50 | Medium |
| Integration wiring (`tui`, `loop_engine`, exports) | ~20 | Trivial |
| Tests | ~220 | Medium |
| **Total** | **~640** | **Medium** |

---

## 8. Future Extensions (Out of Scope for V1)

- Better path canonicalization hook (shared with `jailed_path`) for alias-safe invalidation.
- Command-aware `run_command` invalidation (instead of full flush).
- LRU (recency-based) eviction instead of FIFO oldest eviction.
- Cross-cycle persistent cache with file-change detection (`mtime`/hash).
- Optional faster non-crypto hashing (`ahash`/`foldhash`) after dependency policy decision.
- Precomputed tool-owner index in `SkillRegistry` (e.g., `HashMap<String, usize>`) for O(1) `cacheability()` delegation lookups.

---

## V2: TTL-Based Cross-Cycle Caching (Future)

After V1 is shipped and proven stable in production (correctness, cache hit behavior, and invalidation telemetry), the next evolution path is a TTL-based cache that survives cycle boundaries. This section is intentionally **future-facing** and does **not** change V1 scope or semantics.

### Why this is V2 (not V1)

V1 intentionally minimizes risk by clearing cache state at cycle boundaries and keeping cacheability/invalidation rules simple. That avoids two high-risk failure classes:

1. **Stale data across cycles** (entries living too long without robust expiry/invalidation).
2. **Scope leaks** (reusing entries across incompatible execution contexts).

A cross-cycle cache must prove correctness under both conditions before adoption, so V2 depends on V1 stability and instrumentation first.

### Proposed V2 design

#### Per-tool TTLs

Use explicit TTL windows tuned by tool volatility:

- `read_file`: 5–30s
- `list_directory`: 5–15s
- `search_text`: 5–15s
- `git_status`: 2–5s
- `git_diff`: 2–5s

#### Cache key with scope fingerprint + policy version

Key derivation:

`hash(tool_name + normalized_args + scope_fingerprint + policy_version)`

- `scope_fingerprint`: correctness context (e.g., `cwd`, repo root, session id).
- `policy_version`: explicit version bump to invalidate older key schemes safely.

This protects against cross-context key collisions and supports controlled cache policy migrations.

#### Cross-cycle persistence

Entries survive cycle boundaries and expire by TTL, rather than being fully cleared in `prepare_cycle()`.

#### Capacity controls + eviction

Use LRU eviction when either limit is exceeded:

- max entries
- max bytes

#### Config-driven per-tool policy

Expose per-tool settings via `config.toml`:

```toml
[tool_cache.tools.read_file]
enabled = true
ttl_ms = 10000
```

#### Data structures and metadata

- `hashbrown::HashMap` for entry storage + an LRU index for recency tracking.
- `CacheEntry` metadata includes `created_at`, `expires_at`, `size_bytes`.
- `ToolCacheStats` extends observability with `hits`, `misses`, `writes`, `evictions`, `invalidations`.

#### Telemetry

Prometheus-style counters/histograms for cache behavior, e.g.:

- `tool_cache.hit_total{tool=...}`
- `tool_cache.miss_total{tool=...}`
- `tool_cache.write_total{tool=...}`
- `tool_cache.eviction_total{reason=...}`
- `tool_cache.invalidation_total{reason=...}`

### Attribution

V2 design inspired by **Fawx self-analysis spec**.
