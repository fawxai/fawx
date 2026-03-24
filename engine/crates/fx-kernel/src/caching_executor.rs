use crate::act::{
    ConcurrencyPolicy, ToolCacheStats, ToolCacheability, ToolExecutor, ToolExecutorError,
    ToolResult,
};
use crate::cancellation::CancellationToken;
use async_trait::async_trait;
use fx_llm::{ToolCall, ToolDefinition};
use serde_json::Value;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use tracing::warn;

const MAX_CACHE_ENTRIES: usize = 256;
const RECURSIVE_LIST_INDEX_PREFIX: &str = "list_recursive:";

#[derive(Debug)]
pub struct CachingExecutor<T: ToolExecutor> {
    inner: T,
    cache: Mutex<ToolCache>,
}

#[derive(Debug, Default)]
struct ToolCache {
    entries: HashMap<CacheKey, CachedResult>,
    order: VecDeque<CacheKey>,
    path_index: HashMap<String, HashSet<CacheKey>>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    tool_name: String,
    args_hash: u64,
}

impl CacheKey {
    fn new(tool_name: &str, arguments: &Value) -> Self {
        let mut hasher = DefaultHasher::new();
        normalize_json(arguments).hash(&mut hasher);
        Self {
            tool_name: tool_name.to_string(),
            args_hash: hasher.finish(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedResult {
    output: String,
    success: bool,
    indexed_paths: Vec<String>,
}

#[derive(Debug)]
struct PendingCall {
    original_index: usize,
    call: ToolCall,
    cacheability: ToolCacheability,
    cache_key: Option<CacheKey>,
}

impl PendingCall {
    fn new(original_index: usize, call: &ToolCall, cacheability: ToolCacheability) -> Self {
        let cache_key = if cacheability == ToolCacheability::Cacheable {
            Some(CacheKey::new(&call.name, &call.arguments))
        } else {
            None
        };

        Self {
            original_index,
            call: call.clone(),
            cacheability,
            cache_key,
        }
    }
}

#[derive(Debug)]
struct DeduplicatedCall {
    original_index: usize,
    source_pending_index: usize,
    call: ToolCall,
}

impl DeduplicatedCall {
    fn new(original_index: usize, source_pending_index: usize, call: &ToolCall) -> Self {
        Self {
            original_index,
            source_pending_index,
            call: call.clone(),
        }
    }
}

#[derive(Debug)]
struct ToolExecutionPlan {
    ordered_results: Vec<Option<ToolResult>>,
    pending: Vec<PendingCall>,
    deduplicated: Vec<DeduplicatedCall>,
}

impl ToolCache {
    fn remove_key(&mut self, key: &CacheKey) -> bool {
        let Some(cached) = self.entries.remove(key) else {
            return false;
        };

        self.remove_key_from_order(key);
        self.remove_key_from_path_index(key, &cached.indexed_paths);
        true
    }

    fn remove_key_from_order(&mut self, key: &CacheKey) {
        self.order.retain(|current| current != key);
    }

    fn remove_key_from_path_index(&mut self, key: &CacheKey, paths: &[String]) {
        let mut empty_paths = Vec::new();
        for path in paths {
            if let Some(keys) = self.path_index.get_mut(path) {
                keys.remove(key);
                if keys.is_empty() {
                    empty_paths.push(path.clone());
                }
            }
        }

        for path in empty_paths {
            self.path_index.remove(&path);
        }
    }

    fn invalidate_path(&mut self, path: &str) {
        let Some(keys) = self.path_index.get(path).cloned() else {
            return;
        };

        for key in keys {
            self.remove_key(&key);
        }
    }

    fn invalidate_tool(&mut self, tool_name: &str) {
        let keys = self
            .entries
            .keys()
            .filter(|key| key.tool_name == tool_name)
            .cloned()
            .collect::<Vec<_>>();

        for key in keys {
            self.remove_key(&key);
        }
    }

    fn evict_oldest(&mut self) {
        while let Some(key) = self.order.pop_front() {
            let Some(entry) = self.entries.remove(&key) else {
                continue;
            };

            self.remove_key_from_path_index(&key, &entry.indexed_paths);
            self.evictions = self.evictions.saturating_add(1);
            return;
        }
    }

    fn flush_all_cacheable(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.path_index.clear();
    }
}

impl<T: ToolExecutor> CachingExecutor<T> {
    #[must_use]
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            cache: Mutex::new(ToolCache::default()),
        }
    }

    fn reset_cache_state(&self) {
        let Ok(mut cache) = self.cache.lock() else {
            warn!("tool cache lock poisoned during cache reset; skipping cache reset");
            return;
        };

        cache.entries.clear();
        cache.order.clear();
        cache.path_index.clear();
        cache.hits = 0;
        cache.misses = 0;
        cache.evictions = 0;
    }

    fn lookup(&self, key: &CacheKey) -> Option<CachedResult> {
        let Ok(mut cache) = self.cache.lock() else {
            warn!("tool cache lock poisoned during lookup; treating as cache miss");
            return None;
        };

        let hit = cache.entries.get(key).cloned();
        if hit.is_some() {
            cache.hits = cache.hits.saturating_add(1);
        } else {
            cache.misses = cache.misses.saturating_add(1);
        }

        hit
    }

    fn cache_stats_snapshot(&self) -> Option<ToolCacheStats> {
        let Ok(cache) = self.cache.lock() else {
            warn!("tool cache lock poisoned while reading cache stats; skipping stats emission");
            return None;
        };

        Some(ToolCacheStats {
            hits: cache.hits,
            misses: cache.misses,
            entries: cache.entries.len() as u64,
            evictions: cache.evictions,
        })
    }

    fn plan_calls(&self, calls: &[ToolCall]) -> ToolExecutionPlan {
        let mut ordered_results = vec![None; calls.len()];
        let mut pending = Vec::new();
        let mut deduplicated = Vec::new();
        let mut pending_cache_keys = HashMap::new();

        for (index, call) in calls.iter().enumerate() {
            if let Some(result) = self.resolve_cached_call(call) {
                ordered_results[index] = Some(result);
                continue;
            }

            let pending_call = PendingCall::new(index, call, self.inner.cacheability(&call.name));
            if let Some(cache_key) = pending_call.cache_key.clone() {
                if let Some(source_pending_index) = pending_cache_keys.get(&cache_key).copied() {
                    deduplicated.push(DeduplicatedCall::new(index, source_pending_index, call));
                    continue;
                }
                pending_cache_keys.insert(cache_key, pending.len());
            }

            pending.push(pending_call);
        }

        ToolExecutionPlan {
            ordered_results,
            pending,
            deduplicated,
        }
    }

    fn resolve_cached_call(&self, call: &ToolCall) -> Option<ToolResult> {
        if self.inner.cacheability(&call.name) != ToolCacheability::Cacheable {
            return None;
        }

        let key = CacheKey::new(&call.name, &call.arguments);
        self.lookup(&key).map(|cached| ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: cached.success,
            output: cached.output,
        })
    }

    async fn execute_pending_calls(
        &self,
        pending: &[PendingCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let calls = pending
            .iter()
            .map(|pending_call| pending_call.call.clone())
            .collect::<Vec<_>>();
        self.inner.execute_tools(&calls, cancel).await
    }

    fn apply_executed_results(
        &self,
        plan: &mut ToolExecutionPlan,
        executed_results: Vec<ToolResult>,
    ) -> Result<(), ToolExecutorError> {
        if executed_results.len() != plan.pending.len() {
            return Err(ToolExecutorError {
                message: format!(
                    "caching executor expected {} results, got {}",
                    plan.pending.len(),
                    executed_results.len()
                ),
                recoverable: false,
            });
        }

        let mut pending_results = Vec::with_capacity(plan.pending.len());
        for (pending, result) in plan.pending.iter().zip(executed_results) {
            self.update_cache_for_result(pending, &result);
            plan.ordered_results[pending.original_index] = Some(result.clone());
            pending_results.push(result);
        }

        for deduplicated in &plan.deduplicated {
            let Some(source_result) = pending_results.get(deduplicated.source_pending_index) else {
                return Err(ToolExecutorError {
                    message: format!(
                        "caching executor missing deduplicated source result at slot {}",
                        deduplicated.source_pending_index
                    ),
                    recoverable: false,
                });
            };

            plan.ordered_results[deduplicated.original_index] = Some(result_for_deduplicated_call(
                source_result,
                &deduplicated.call,
            ));
        }

        Ok(())
    }

    fn update_cache_for_result(&self, pending: &PendingCall, result: &ToolResult) {
        match pending.cacheability {
            ToolCacheability::Cacheable if result.success => {
                if let Some(key) = pending.cache_key.clone() {
                    self.store(key, &pending.call.name, &pending.call.arguments, result);
                }
            }
            ToolCacheability::SideEffect => {
                self.invalidate_for_side_effect(&pending.call.name, &pending.call.arguments);
            }
            ToolCacheability::NeverCache | ToolCacheability::Cacheable => {}
        }
    }

    fn store(&self, key: CacheKey, tool_name: &str, arguments: &Value, result: &ToolResult) {
        let indexed_paths = extract_index_paths(tool_name, arguments);
        let Ok(mut cache) = self.cache.lock() else {
            warn!("tool cache lock poisoned during store; skipping cache write");
            return;
        };

        cache.remove_key(&key);
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

    fn invalidate_for_side_effect(&self, tool_name: &str, arguments: &Value) {
        let Ok(mut cache) = self.cache.lock() else {
            warn!("tool cache lock poisoned during invalidation; skipping invalidation");
            return;
        };

        match tool_name {
            "write_file" => invalidate_write_file_cache(&mut cache, arguments),
            "memory_write" | "memory_delete" => {
                invalidate_memory_cache(&mut cache, arguments);
            }
            "run_command" => cache.flush_all_cacheable(),
            _ => {}
        }
    }
}

#[async_trait]
impl<T: ToolExecutor> ToolExecutor for CachingExecutor<T> {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let mut plan = self.plan_calls(calls);

        if !plan.pending.is_empty() {
            let executed_results = self.execute_pending_calls(&plan.pending, cancel).await?;
            self.apply_executed_results(&mut plan, executed_results)?;
        }

        collect_ordered_results(plan.ordered_results)
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

fn collect_ordered_results(
    ordered_results: Vec<Option<ToolResult>>,
) -> Result<Vec<ToolResult>, ToolExecutorError> {
    ordered_results
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            result.ok_or_else(|| ToolExecutorError {
                message: format!("caching executor missing tool result at slot {index}"),
                recoverable: false,
            })
        })
        .collect()
}

fn result_for_deduplicated_call(source: &ToolResult, call: &ToolCall) -> ToolResult {
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: source.success,
        output: source.output.clone(),
    }
}

fn invalidate_write_file_cache(cache: &mut ToolCache, arguments: &Value) {
    let Some(path) = json_path_arg(arguments, "path") else {
        return;
    };

    cache.invalidate_path(&path);
    if let Some(parent_path) = parent_path(&path) {
        cache.invalidate_path(&parent_path);
        invalidate_recursive_list_cache(cache, &parent_path);
    }
    cache.invalidate_tool("search_text");
}

fn invalidate_recursive_list_cache(cache: &mut ToolCache, path: &str) {
    for ancestor in ancestor_paths(path) {
        cache.invalidate_path(&recursive_list_index_key(&ancestor));
    }
}

fn ancestor_paths(path: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut cursor = PathBuf::from(path);

    loop {
        let current = cursor.to_string_lossy().to_string();
        if !current.is_empty() {
            ancestors.push(current);
        }

        let Some(parent) = cursor.parent() else {
            break;
        };
        if parent.as_os_str().is_empty() {
            if Path::new(path).is_relative() && !ancestors.iter().any(|entry| entry == ".") {
                ancestors.push(".".to_string());
            }
            break;
        }

        let parent_buf = parent.to_path_buf();
        if parent_buf == cursor {
            break;
        }
        cursor = parent_buf;
    }

    ancestors
}

fn parent_path(path: &str) -> Option<String> {
    let parent = Path::new(path).parent()?;
    if parent.as_os_str().is_empty() {
        return Path::new(path).is_relative().then(|| ".".to_string());
    }
    Some(normalize_path(&parent.to_string_lossy()))
}

fn invalidate_memory_cache(cache: &mut ToolCache, arguments: &Value) {
    if let Some(key) = json_string_arg(arguments, "key") {
        cache.invalidate_path(&format!("memory:{key}"));
    }
    cache.invalidate_path("memory:*");
}

fn json_string_arg(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn json_bool_arg(arguments: &Value, key: &str) -> Option<bool> {
    arguments.get(key).and_then(Value::as_bool)
}

fn json_path_arg(arguments: &Value, key: &str) -> Option<String> {
    json_string_arg(arguments, key).map(|path| normalize_path(&path))
}

fn extract_index_paths(tool_name: &str, arguments: &Value) -> Vec<String> {
    match tool_name {
        "read_file" | "search_text" => json_path_arg(arguments, "path").into_iter().collect(),
        "list_directory" => list_directory_index_paths(arguments),
        "memory_read" => json_string_arg(arguments, "key")
            .map(|key| vec![format!("memory:{key}")])
            .unwrap_or_default(),
        "memory_list" => vec!["memory:*".to_string()],
        _ => Vec::new(),
    }
}

fn list_directory_index_paths(arguments: &Value) -> Vec<String> {
    let Some(path) = json_path_arg(arguments, "path") else {
        return Vec::new();
    };

    let mut indexed = vec![path.clone()];
    if json_bool_arg(arguments, "recursive") == Some(true) {
        indexed.push(recursive_list_index_key(&path));
    }
    indexed
}

fn recursive_list_index_key(path: &str) -> String {
    format!("{RECURSIVE_LIST_INDEX_PREFIX}{path}")
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    let (prefix, rooted, parts) = normalized_path_parts(path);
    render_normalized_path(prefix, rooted, parts)
}

fn normalized_path_parts(path: &str) -> (Option<String>, bool, Vec<String>) {
    let mut prefix = None;
    let mut rooted = false;
    let mut parts = Vec::new();

    for component in Path::new(path).components() {
        apply_path_component(component, &mut prefix, &mut rooted, &mut parts);
    }

    (prefix, rooted, parts)
}

fn apply_path_component(
    component: Component<'_>,
    prefix: &mut Option<String>,
    rooted: &mut bool,
    parts: &mut Vec<String>,
) {
    match component {
        Component::Prefix(value) => {
            *prefix = Some(value.as_os_str().to_string_lossy().to_string());
        }
        Component::RootDir => {
            *rooted = true;
            parts.clear();
        }
        Component::CurDir => {}
        Component::ParentDir => pop_or_push_parent(parts, *rooted),
        Component::Normal(segment) => {
            parts.push(segment.to_string_lossy().to_string());
        }
    }
}

fn pop_or_push_parent(parts: &mut Vec<String>, rooted: bool) {
    if parts.last().is_some_and(|segment| segment != "..") {
        parts.pop();
    } else if !rooted {
        parts.push("..".to_string());
    }
}

fn render_normalized_path(prefix: Option<String>, rooted: bool, parts: Vec<String>) -> String {
    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if rooted {
        normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    }
    for part in parts {
        normalized.push(part);
    }

    let rendered = normalized.to_string_lossy().to_string();
    if rendered.is_empty() {
        ".".to_string()
    } else {
        rendered
    }
}

fn normalize_json(value: &Value) -> String {
    normalize_json_value(value).to_string()
}

fn normalize_json_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => normalize_json_object(map),
        Value::Array(items) => Value::Array(items.iter().map(normalize_json_value).collect()),
        _ => value.clone(),
    }
}

fn normalize_json_object(map: &serde_json::Map<String, Value>) -> Value {
    let mut keys = map.keys().cloned().collect::<Vec<_>>();
    keys.sort();

    let mut normalized = serde_json::Map::with_capacity(map.len());
    for key in keys {
        if let Some(child) = map.get(&key) {
            normalized.insert(key.clone(), normalize_json_field(&key, child));
        }
    }

    Value::Object(normalized)
}

fn normalize_json_field(key: &str, value: &Value) -> Value {
    if key == "path" {
        if let Some(path) = value.as_str() {
            return Value::String(normalize_path(path));
        }
    }

    normalize_json_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    #[derive(Debug, Default)]
    struct MockState {
        calls: Mutex<Vec<ToolCall>>,
    }

    #[derive(Debug, Clone)]
    struct MockToolExecutor {
        state: Arc<MockState>,
        cacheability: HashMap<String, ToolCacheability>,
        failing_tools: HashSet<String>,
        definitions: Vec<ToolDefinition>,
        policy: ConcurrencyPolicy,
    }

    impl MockToolExecutor {
        fn new(cacheability: HashMap<String, ToolCacheability>) -> Self {
            Self {
                state: Arc::new(MockState::default()),
                cacheability,
                failing_tools: HashSet::new(),
                definitions: vec![tool_definition("read_file")],
                policy: ConcurrencyPolicy::default(),
            }
        }

        fn with_failing_tool(mut self, tool_name: &str) -> Self {
            self.failing_tools.insert(tool_name.to_string());
            self
        }

        fn with_definitions(mut self, definitions: Vec<ToolDefinition>) -> Self {
            self.definitions = definitions;
            self
        }

        fn with_policy(mut self, policy: ConcurrencyPolicy) -> Self {
            self.policy = policy;
            self
        }

        fn calls_for_tool(&self, tool_name: &str) -> usize {
            self.calls_matching(tool_name, |_, _| true)
        }

        fn calls_for_tool_path(&self, tool_name: &str, path: &str) -> usize {
            self.calls_matching(tool_name, |call, _| call_path(call) == Some(path))
        }

        fn calls_for_tool_key(&self, tool_name: &str, key: &str) -> usize {
            self.calls_matching(tool_name, |call, _| call_key(call) == Some(key))
        }

        fn calls_matching<F>(&self, tool_name: &str, matcher: F) -> usize
        where
            F: Fn(&ToolCall, usize) -> bool,
        {
            self.state
                .calls
                .lock()
                .expect("mock state lock")
                .iter()
                .enumerate()
                .filter(|(index, call)| call.name == tool_name && matcher(call, *index))
                .count()
        }
    }

    #[async_trait]
    impl ToolExecutor for MockToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            self.state
                .calls
                .lock()
                .expect("mock state lock")
                .extend(calls.iter().cloned());

            let results = calls.iter().map(|call| self.build_result(call)).collect();
            Ok(results)
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.definitions.clone()
        }

        fn cacheability(&self, tool_name: &str) -> ToolCacheability {
            self.cacheability
                .get(tool_name)
                .copied()
                .unwrap_or(ToolCacheability::NeverCache)
        }

        fn concurrency_policy(&self) -> ConcurrencyPolicy {
            self.policy.clone()
        }
    }

    impl MockToolExecutor {
        fn build_result(&self, call: &ToolCall) -> ToolResult {
            let failed = self.failing_tools.contains(&call.name);
            let output = if failed {
                format!("error:{}", call.name)
            } else {
                format!("{}:{}", call.name, normalize_json(&call.arguments))
            };

            ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: !failed,
                output,
            }
        }
    }

    fn tool_definition(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("{name} tool"),
            parameters: serde_json::json!({"type":"object"}),
        }
    }

    fn cacheability_map(entries: &[(&str, ToolCacheability)]) -> HashMap<String, ToolCacheability> {
        entries
            .iter()
            .map(|(name, cacheability)| ((*name).to_string(), *cacheability))
            .collect()
    }

    fn tool_call(id: &str, name: &str, arguments: Value) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments,
        }
    }

    fn read_call(id: &str, path: &str) -> ToolCall {
        tool_call(id, "read_file", serde_json::json!({"path": path}))
    }

    fn list_call(id: &str, path: &str) -> ToolCall {
        tool_call(id, "list_directory", serde_json::json!({"path": path}))
    }

    fn list_recursive_call(id: &str, path: &str) -> ToolCall {
        tool_call(
            id,
            "list_directory",
            serde_json::json!({"path": path, "recursive": true}),
        )
    }

    fn write_call(id: &str, path: &str) -> ToolCall {
        tool_call(
            id,
            "write_file",
            serde_json::json!({"path": path, "content": "x"}),
        )
    }

    fn memory_read_call(id: &str, key: &str) -> ToolCall {
        tool_call(id, "memory_read", serde_json::json!({"key": key}))
    }

    fn memory_list_call(id: &str) -> ToolCall {
        tool_call(id, "memory_list", serde_json::json!({}))
    }

    fn memory_write_call(id: &str, key: &str) -> ToolCall {
        tool_call(
            id,
            "memory_write",
            serde_json::json!({"key": key, "value": "next"}),
        )
    }

    fn call_path(call: &ToolCall) -> Option<&str> {
        call.arguments.get("path").and_then(Value::as_str)
    }

    fn call_key(call: &ToolCall) -> Option<&str> {
        call.arguments.get("key").and_then(Value::as_str)
    }

    #[tokio::test]
    async fn cache_hit_returns_stored_result() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        let first = executor
            .execute_tools(&[read_call("1", "Cargo.toml")], None)
            .await
            .expect("first call");
        let second = executor
            .execute_tools(&[read_call("2", "Cargo.toml")], None)
            .await
            .expect("second call");

        assert_eq!(first[0].output, second[0].output);
        assert_eq!(second[0].tool_call_id, "2");
        assert_eq!(probe.calls_for_tool_path("read_file", "Cargo.toml"), 1);
    }

    #[tokio::test]
    async fn cache_miss_for_different_args() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("1", "a.txt")], None)
            .await
            .expect("first miss");
        executor
            .execute_tools(&[read_call("2", "b.txt")], None)
            .await
            .expect("second miss");

        assert_eq!(probe.calls_for_tool("read_file"), 2);
    }

    #[tokio::test]
    async fn cache_miss_for_different_tools() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("read_file", ToolCacheability::Cacheable),
            ("list_directory", ToolCacheability::Cacheable),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("1", "/tmp")], None)
            .await
            .expect("read miss");
        executor
            .execute_tools(&[list_call("2", "/tmp")], None)
            .await
            .expect("list miss");

        assert_eq!(probe.calls_for_tool("read_file"), 1);
        assert_eq!(probe.calls_for_tool("list_directory"), 1);
    }

    #[tokio::test]
    async fn never_cache_tool_always_executes() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "current_time",
            ToolCacheability::NeverCache,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        let call = tool_call("1", "current_time", serde_json::json!({}));
        executor
            .execute_tools(std::slice::from_ref(&call), None)
            .await
            .expect("first current_time");
        executor
            .execute_tools(
                &[tool_call("2", "current_time", serde_json::json!({}))],
                None,
            )
            .await
            .expect("second current_time");

        assert_eq!(probe.calls_for_tool("current_time"), 2);
    }

    #[tokio::test]
    async fn side_effect_tool_not_cached() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "write_file",
            ToolCacheability::SideEffect,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[write_call("1", "notes.txt")], None)
            .await
            .expect("first write");
        executor
            .execute_tools(&[write_call("2", "notes.txt")], None)
            .await
            .expect("second write");

        assert_eq!(probe.calls_for_tool_path("write_file", "notes.txt"), 2);
    }

    #[tokio::test]
    async fn write_file_invalidates_read_file_via_path_index() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("read_file", ToolCacheability::Cacheable),
            ("write_file", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("1", "/tmp/a.txt")], None)
            .await
            .expect("warm cache");
        executor
            .execute_tools(&[write_call("2", "/tmp/a.txt")], None)
            .await
            .expect("write invalidates");
        executor
            .execute_tools(&[read_call("3", "/tmp/a.txt")], None)
            .await
            .expect("read after invalidation");

        assert_eq!(probe.calls_for_tool_path("read_file", "/tmp/a.txt"), 2);
    }

    #[tokio::test]
    async fn write_file_invalidates_parent_list_directory_via_path_index() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("list_directory", ToolCacheability::Cacheable),
            ("write_file", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[list_call("1", "/tmp/project")], None)
            .await
            .expect("warm list cache");
        executor
            .execute_tools(&[write_call("2", "/tmp/project/file.txt")], None)
            .await
            .expect("write invalidates parent list");
        executor
            .execute_tools(&[list_call("3", "/tmp/project")], None)
            .await
            .expect("list after invalidation");

        assert_eq!(
            probe.calls_for_tool_path("list_directory", "/tmp/project"),
            2
        );
    }

    #[tokio::test]
    async fn write_file_invalidates_recursive_ancestor_list_directory() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("list_directory", ToolCacheability::Cacheable),
            ("write_file", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[list_recursive_call("1", "/tmp/project")], None)
            .await
            .expect("warm recursive list cache");
        executor
            .execute_tools(&[write_call("2", "/tmp/project/src/new/file.txt")], None)
            .await
            .expect("write invalidates recursive ancestors");
        executor
            .execute_tools(&[list_recursive_call("3", "/tmp/project")], None)
            .await
            .expect("recursive list after invalidation");

        assert_eq!(
            probe.calls_for_tool_path("list_directory", "/tmp/project"),
            2
        );
    }

    #[tokio::test]
    async fn write_file_nested_relative_path_invalidates_recursive_list_from_relative_root() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("list_directory", ToolCacheability::Cacheable),
            ("write_file", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[list_recursive_call("1", ".")], None)
            .await
            .expect("warm recursive root list");
        executor
            .execute_tools(&[write_call("2", "src/new/file.txt")], None)
            .await
            .expect("write invalidates relative ancestors");
        executor
            .execute_tools(&[list_recursive_call("3", ".")], None)
            .await
            .expect("recursive root list after invalidation");

        assert_eq!(probe.calls_for_tool_path("list_directory", "."), 2);
    }

    #[tokio::test]
    async fn write_file_root_relative_path_invalidates_recursive_list_from_relative_root() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("list_directory", ToolCacheability::Cacheable),
            ("write_file", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[list_recursive_call("1", ".")], None)
            .await
            .expect("warm recursive root list");
        executor
            .execute_tools(&[write_call("2", "foo.txt")], None)
            .await
            .expect("write invalidates root-relative file");
        executor
            .execute_tools(&[list_recursive_call("3", ".")], None)
            .await
            .expect("recursive root list after invalidation");

        assert_eq!(probe.calls_for_tool_path("list_directory", "."), 2);
    }

    #[tokio::test]
    async fn normalized_paths_share_cache_keys_and_invalidation() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("read_file", ToolCacheability::Cacheable),
            ("write_file", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        let first = executor
            .execute_tools(&[read_call("1", "/tmp/project/./notes.txt")], None)
            .await
            .expect("warm normalized read cache");
        let second = executor
            .execute_tools(&[read_call("2", "/tmp/project/notes.txt")], None)
            .await
            .expect("equivalent path cache hit");

        assert_eq!(first[0].output, second[0].output);
        assert_eq!(probe.calls_for_tool("read_file"), 1);

        executor
            .execute_tools(&[write_call("3", "/tmp/project/dir/../notes.txt")], None)
            .await
            .expect("write invalidates normalized path key");
        executor
            .execute_tools(&[read_call("4", "/tmp/project/notes.txt")], None)
            .await
            .expect("read after normalized invalidation");

        assert_eq!(probe.calls_for_tool("read_file"), 2);
    }

    #[tokio::test]
    async fn memory_write_invalidates_memory_read_and_list() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("memory_read", ToolCacheability::Cacheable),
            ("memory_list", ToolCacheability::Cacheable),
            ("memory_write", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[memory_read_call("1", "topic")], None)
            .await
            .expect("warm memory_read");
        executor
            .execute_tools(&[memory_list_call("2")], None)
            .await
            .expect("warm memory_list");
        executor
            .execute_tools(&[memory_write_call("3", "topic")], None)
            .await
            .expect("memory write invalidates");
        executor
            .execute_tools(&[memory_read_call("4", "topic")], None)
            .await
            .expect("memory_read after invalidation");
        executor
            .execute_tools(&[memory_list_call("5")], None)
            .await
            .expect("memory_list after invalidation");

        assert_eq!(probe.calls_for_tool_key("memory_read", "topic"), 2);
        assert_eq!(probe.calls_for_tool("memory_list"), 2);
    }

    #[tokio::test]
    async fn run_command_flushes_cacheable_entries() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("read_file", ToolCacheability::Cacheable),
            ("list_directory", ToolCacheability::Cacheable),
            ("run_command", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("1", "/tmp/a.txt")], None)
            .await
            .expect("warm read cache");
        executor
            .execute_tools(&[list_call("2", "/tmp")], None)
            .await
            .expect("warm list cache");
        executor
            .execute_tools(
                &[tool_call(
                    "3",
                    "run_command",
                    serde_json::json!({"command":"echo hi"}),
                )],
                None,
            )
            .await
            .expect("run command flushes cache");
        executor
            .execute_tools(&[read_call("4", "/tmp/a.txt")], None)
            .await
            .expect("read after flush");
        executor
            .execute_tools(&[list_call("5", "/tmp")], None)
            .await
            .expect("list after flush");

        assert_eq!(probe.calls_for_tool_path("read_file", "/tmp/a.txt"), 2);
        assert_eq!(probe.calls_for_tool_path("list_directory", "/tmp"), 2);
    }

    #[tokio::test]
    async fn clear_cache_resets_entries_indexes_and_stats() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("1", "Cargo.toml")], None)
            .await
            .expect("warm cache");
        executor
            .execute_tools(&[read_call("2", "Cargo.toml")], None)
            .await
            .expect("cache hit");
        executor.clear_cache();

        let stats = executor.cache_stats().expect("cache stats");
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.evictions, 0);

        executor
            .execute_tools(&[read_call("3", "Cargo.toml")], None)
            .await
            .expect("miss after clear");
        assert_eq!(probe.calls_for_tool_path("read_file", "Cargo.toml"), 2);
    }

    #[tokio::test]
    async fn cache_stats_tracks_hits_misses_evictions() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]));
        let executor = CachingExecutor::new(inner);

        for index in 0..MAX_CACHE_ENTRIES {
            let path = format!("file-{index}.txt");
            executor
                .execute_tools(&[read_call("warm", &path)], None)
                .await
                .expect("warm cache entry");
        }

        executor
            .execute_tools(&[read_call("hit", "file-0.txt")], None)
            .await
            .expect("cache hit");
        executor
            .execute_tools(&[read_call("overflow", "overflow.txt")], None)
            .await
            .expect("overflow insert");
        executor
            .execute_tools(&[read_call("miss", "file-0.txt")], None)
            .await
            .expect("miss after eviction");

        let stats = executor.cache_stats().expect("cache stats");
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, MAX_CACHE_ENTRIES as u64 + 2);
        assert_eq!(stats.entries, MAX_CACHE_ENTRIES as u64);
        assert_eq!(stats.evictions, 2);
    }

    #[test]
    fn json_normalization_matches_reordered_keys() {
        let first = serde_json::json!({
            "b": 2,
            "a": {"d": 4, "c": 3},
            "items": [{"y": 2, "x": 1}],
        });
        let second = serde_json::json!({
            "items": [{"x": 1, "y": 2}],
            "a": {"c": 3, "d": 4},
            "b": 2,
        });

        assert_eq!(normalize_json(&first), normalize_json(&second));
        assert_eq!(
            CacheKey::new("read_file", &first),
            CacheKey::new("read_file", &second)
        );
    }

    #[tokio::test]
    async fn failed_tool_result_not_cached() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]))
        .with_failing_tool("read_file");
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("1", "Cargo.toml")], None)
            .await
            .expect("first failure");
        executor
            .execute_tools(&[read_call("2", "Cargo.toml")], None)
            .await
            .expect("second failure");

        let stats = executor.cache_stats().expect("cache stats");
        assert_eq!(probe.calls_for_tool_path("read_file", "Cargo.toml"), 2);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2);
        assert_eq!(stats.entries, 0);
    }

    #[tokio::test]
    async fn oldest_entry_evicted_when_capacity_exceeded() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        for index in 0..MAX_CACHE_ENTRIES {
            let path = format!("item-{index}.txt");
            executor
                .execute_tools(&[read_call("seed", &path)], None)
                .await
                .expect("seed cache entry");
        }

        executor
            .execute_tools(&[read_call("overflow", "item-overflow.txt")], None)
            .await
            .expect("overflow insert");
        executor
            .execute_tools(&[read_call("oldest", "item-0.txt")], None)
            .await
            .expect("oldest should miss after eviction");

        let stats = executor.cache_stats().expect("cache stats");
        assert!(stats.evictions >= 1);
        assert_eq!(probe.calls_for_tool_path("read_file", "item-0.txt"), 2);
    }

    #[tokio::test]
    async fn ordered_results_preserve_call_order_with_mixed_hits_and_misses() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("warm", "A.txt")], None)
            .await
            .expect("warm cache");

        let calls = vec![
            read_call("1", "A.txt"),
            read_call("2", "B.txt"),
            read_call("3", "A.txt"),
        ];
        let results = executor
            .execute_tools(&calls, None)
            .await
            .expect("batch result");

        let ids = results
            .iter()
            .map(|result| result.tool_call_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["1", "2", "3"]);
        assert_eq!(probe.calls_for_tool_path("read_file", "A.txt"), 1);
        assert_eq!(probe.calls_for_tool_path("read_file", "B.txt"), 1);
    }

    #[tokio::test]
    async fn cold_batch_deduplicates_identical_cacheable_calls() {
        let inner = MockToolExecutor::new(cacheability_map(&[(
            "read_file",
            ToolCacheability::Cacheable,
        )]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        let calls = vec![
            read_call("1", "/tmp/a.txt"),
            read_call("2", "/tmp/a.txt"),
            read_call("3", "/tmp/a.txt"),
        ];
        let results = executor
            .execute_tools(&calls, None)
            .await
            .expect("deduplicated cold batch result");

        let ids = results
            .iter()
            .map(|result| result.tool_call_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["1", "2", "3"]);
        assert_eq!(probe.calls_for_tool_path("read_file", "/tmp/a.txt"), 1);
        assert_eq!(results[0].output, results[1].output);
        assert_eq!(results[1].output, results[2].output);
    }

    #[tokio::test]
    async fn mixed_cacheability_batch_preserves_order_and_semantics() {
        let inner = MockToolExecutor::new(cacheability_map(&[
            ("read_file", ToolCacheability::Cacheable),
            ("current_time", ToolCacheability::NeverCache),
            ("write_file", ToolCacheability::SideEffect),
        ]));
        let probe = inner.clone();
        let executor = CachingExecutor::new(inner);

        executor
            .execute_tools(&[read_call("warm", "/tmp/a.txt")], None)
            .await
            .expect("warm cache");

        let batch = vec![
            read_call("1", "/tmp/a.txt"),
            tool_call("2", "current_time", serde_json::json!({})),
            write_call("3", "/tmp/a.txt"),
            tool_call("4", "current_time", serde_json::json!({})),
        ];
        let first_batch = executor
            .execute_tools(&batch, None)
            .await
            .expect("batch result");

        let ids = first_batch
            .iter()
            .map(|result| result.tool_call_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["1", "2", "3", "4"]);

        executor
            .execute_tools(&[read_call("5", "/tmp/a.txt")], None)
            .await
            .expect("read after side effect");

        assert_eq!(probe.calls_for_tool_path("read_file", "/tmp/a.txt"), 2);
        assert_eq!(probe.calls_for_tool("current_time"), 2);
        assert_eq!(probe.calls_for_tool_path("write_file", "/tmp/a.txt"), 1);
    }

    #[test]
    fn tool_definitions_delegated() {
        let definitions = vec![tool_definition("alpha"), tool_definition("beta")];
        let inner = MockToolExecutor::new(HashMap::new()).with_definitions(definitions.clone());
        let executor = CachingExecutor::new(inner);

        assert_eq!(executor.tool_definitions(), definitions);
    }

    #[test]
    fn concurrency_policy_delegated() {
        let policy = ConcurrencyPolicy {
            max_parallel: NonZeroUsize::new(2),
            timeout_per_call: Some(std::time::Duration::from_secs(5)),
        };
        let inner = MockToolExecutor::new(HashMap::new()).with_policy(policy.clone());
        let executor = CachingExecutor::new(inner);

        let delegated = executor.concurrency_policy();
        assert_eq!(delegated.max_parallel, policy.max_parallel);
        assert_eq!(delegated.timeout_per_call, policy.timeout_per_call);
    }
}
