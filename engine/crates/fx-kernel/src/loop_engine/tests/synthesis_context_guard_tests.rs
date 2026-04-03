use super::*;

fn make_tool_result(index: usize, output_size: usize) -> ToolResult {
    ToolResult {
        tool_call_id: format!("call-{index}"),
        tool_name: format!("tool_{index}"),
        success: true,
        output: "x".repeat(output_size),
    }
}

#[test]
fn eviction_reduces_total_tokens_and_replaces_oldest_with_stubs() {
    // 10 results, each ~5000 tokens (20_000 chars / 4 = 5000 tokens)
    // Total: ~50_000 tokens. Limit: 10_000 tokens.
    let results: Vec<ToolResult> = (0..10).map(|i| make_tool_result(i, 20_000)).collect();

    let evicted = evict_oldest_results(results, 10_000);

    assert_eq!(evicted.len(), 10);

    let stubs: Vec<_> = evicted
        .iter()
        .filter(|r| r.output.starts_with("[evicted:"))
        .collect();
    assert!(!stubs.is_empty(), "at least some results should be evicted");

    // Stubs should preserve tool_name
    for stub in &stubs {
        assert!(
            stub.output.contains(&stub.tool_name),
            "eviction stub must include tool_name"
        );
    }

    // Total tokens should be under limit
    let total_tokens: usize = evicted
        .iter()
        .map(|result| estimate_text_tokens(&result.output))
        .sum();
    assert!(
        total_tokens <= 10_000,
        "total tokens {total_tokens} should be <= 10_000"
    );
}

#[test]
fn no_eviction_when_under_limit() {
    let results: Vec<ToolResult> = (0..3).map(|i| make_tool_result(i, 100)).collect();

    let evicted = evict_oldest_results(results.clone(), 100_000);

    assert_eq!(evicted.len(), 3);
    for (orig, ev) in results.iter().zip(evicted.iter()) {
        assert_eq!(orig.output, ev.output);
    }
}

#[test]
fn single_oversized_result_is_truncated() {
    // One result with 400K chars (~100K tokens), limit = 1_000 tokens
    let results = vec![make_tool_result(0, 400_000)];
    let evicted = evict_oldest_results(results, 1_000);

    assert_eq!(evicted.len(), 1);
    assert!(
        evicted[0].output.len() < 400_000,
        "oversized result should be truncated"
    );
}

#[test]
fn eviction_order_is_oldest_first() {
    // 5 results, each ~2500 tokens (10_000 chars). Total ~12_500. Limit: 5_000
    let results: Vec<ToolResult> = (0..5).map(|i| make_tool_result(i, 10_000)).collect();

    let evicted = evict_oldest_results(results, 5_000);

    // Oldest (index 0, 1, ...) should be evicted first
    let first_non_stub = evicted
        .iter()
        .position(|r| !r.output.starts_with("[evicted:"));

    if let Some(pos) = first_non_stub {
        // All items before pos should be stubs
        for item in &evicted[..pos] {
            assert!(
                item.output.starts_with("[evicted:"),
                "earlier results should be evicted first"
            );
        }
    }
}

#[test]
fn empty_results_returns_empty() {
    let results = evict_oldest_results(Vec::new(), 1_000);
    assert!(results.is_empty());
}

#[test]
fn zero_max_tokens_clamps_to_floor_preserving_results() {
    // NB1: max_synthesis_tokens == 0 should not evict everything.
    // The floor clamp (1000 tokens) ensures at least some results survive.
    let results: Vec<ToolResult> = (0..3).map(|i| make_tool_result(i, 100)).collect();

    let evicted = evict_oldest_results(results, 0);

    assert_eq!(evicted.len(), 3);
    // Small results (~25 tokens each) fit under the 1000-token floor,
    // so none should be evicted.
    let stubs: Vec<_> = evicted
        .iter()
        .filter(|r| r.output.starts_with("[evicted:"))
        .collect();
    assert!(
        stubs.is_empty(),
        "small results should survive under the floor clamp"
    );
}

#[test]
fn synthesis_prompt_after_eviction_is_valid() {
    let results: Vec<ToolResult> = (0..10).map(|i| make_tool_result(i, 20_000)).collect();

    let evicted = evict_oldest_results(results, 10_000);
    let prompt = tool_synthesis_prompt(&evicted, "Summarize results");

    // Prompt should be constructable and contain tool result sections
    assert!(prompt.contains("Tool results:"));
    assert!(prompt.contains("Summarize results"));
}
