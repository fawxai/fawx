use super::*;

mod cancellation_tests;
mod context_compaction_tests;
mod decompose_gate_tests;
mod decomposition_tests;
mod error_path_coverage_tests;
mod kernel_loadable_boundary_tests;
mod loop_resilience_tests;
mod observation_signal_tests;
mod orchestrator_flow_tests;
mod orchestrator_prompt_tests;
mod signal_store_tests;
mod streaming_review_tests;
mod synthesis_context_guard_tests;
mod test_fixtures;
mod tool_round_tests;
mod transition_table_tests;

#[test]
fn normalize_tool_failure_output_normalizes_whitespace_and_digits() {
    let cases = [
        ("error at line 42 pid 1234", "error at line 0 pid 0"),
        (
            "  failed\tat /tmp/build-9\npid 77  ",
            "failed at /tmp/build-0 pid 0",
        ),
        ("retry 007 after 88ms", "retry 0 after 0ms"),
        ("", ""),
        ("   \n\t  ", ""),
    ];

    for (input, expected) in cases {
        assert_eq!(normalize_tool_failure_output(input), expected);
    }
}
