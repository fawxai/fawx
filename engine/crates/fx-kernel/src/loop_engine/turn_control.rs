//! Typed turn-control decisions for the loop engine.
//!
//! This module is the kernel-owned authority for continuation vs. terminal
//! transitions. Profile routing, tool execution, and streaming may provide
//! facts, but they should not independently decide whether a turn is complete.

use crate::act::ContinuationToolScope;

use super::{TaskPhase, TurnExecutionProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalAnswerDecision {
    Accept,
    Continue(ContinueReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinueReason {
    ScopedToolSurface,
    NonTerminalProfile,
    GatheringEvidence,
    NoSideEffectProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ToolContinuationFacts<'a> {
    pub(super) turn_execution_profile: &'a TurnExecutionProfile,
    pub(super) task_phase: Option<TaskPhase>,
    pub(super) next_tool_scope: Option<&'a ContinuationToolScope>,
    pub(super) used_mutation_tools: bool,
    pub(super) observation_pressure_saturated: bool,
    pub(super) mutation_tools_available: bool,
    pub(super) artifact_write_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObservationFollowUpDecision {
    Continue,
    RequireMutationOnly,
    AllowSynthesis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolEvidenceTerminalDecision {
    ContinueRootSynthesis,
    ContinuePolicyDeferred,
    CompleteDirectInspectionEmpty,
    IncompletePendingToolCalls,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingToolCallState {
    None,
    /// The model requested more tools during a normal tool transaction boundary.
    /// This remains incomplete because the controller cannot safely pretend the
    /// requested work was optional.
    UnresolvedIntent,
    /// The model requested more exploratory tools, but a resource boundary ended
    /// the transaction after usable evidence had already been gathered. Root
    /// synthesis may answer from observed evidence while explicitly noting that
    /// the pending request did not run.
    ResourceBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ToolEvidenceTerminalFacts {
    pub(super) has_tool_results: bool,
    pub(super) has_policy_deferred_results: bool,
    pub(super) pending_tool_calls: PendingToolCallState,
    pub(super) direct_inspection: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalResponseValidationOutcome {
    Accept,
    Retry(FinalResponseViolation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalResponseViolation {
    ToolActivityAttempted,
    TruncatedResponse,
    EmptyResponse,
    ProgressOnlyResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FinalResponseValidationFacts<'a> {
    pub(super) attempted_tool_activity: bool,
    pub(super) response_truncated: bool,
    pub(super) response_text: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ObservationFollowUpFacts<'a> {
    pub(super) turn_execution_profile: &'a TurnExecutionProfile,
    pub(super) used_observation_tools: bool,
    pub(super) used_mutation_tools: bool,
    pub(super) observation_pressure_saturated: bool,
    pub(super) mutation_tools_available: bool,
    pub(super) artifact_write_pending: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct TurnControlPlane;

impl TurnControlPlane {
    pub(super) fn decide_tool_continuation(
        facts: ToolContinuationFacts<'_>,
    ) -> FinalAnswerDecision {
        if facts.next_tool_scope.is_some() {
            return FinalAnswerDecision::Continue(ContinueReason::ScopedToolSurface);
        }

        if facts.turn_execution_profile.completes_terminally() {
            return FinalAnswerDecision::Accept;
        }

        if !matches!(facts.turn_execution_profile, TurnExecutionProfile::Standard) {
            return FinalAnswerDecision::Continue(ContinueReason::NonTerminalProfile);
        }

        if facts.task_phase == Some(TaskPhase::Gathering) {
            return FinalAnswerDecision::Continue(ContinueReason::GatheringEvidence);
        }

        if facts.used_mutation_tools {
            return FinalAnswerDecision::Accept;
        }

        if facts.artifact_write_pending && facts.mutation_tools_available {
            return FinalAnswerDecision::Continue(ContinueReason::NoSideEffectProgress);
        }

        FinalAnswerDecision::Accept
    }

    pub(super) fn decide_observation_follow_up(
        facts: ObservationFollowUpFacts<'_>,
    ) -> ObservationFollowUpDecision {
        if facts.turn_execution_profile.owns_tool_surface()
            || !facts.used_observation_tools
            || facts.used_mutation_tools
        {
            return ObservationFollowUpDecision::Continue;
        }

        if facts.artifact_write_pending && facts.mutation_tools_available {
            return ObservationFollowUpDecision::RequireMutationOnly;
        }

        if facts.observation_pressure_saturated {
            ObservationFollowUpDecision::AllowSynthesis
        } else {
            ObservationFollowUpDecision::Continue
        }
    }

    pub(super) fn decide_tool_evidence_terminal(
        facts: ToolEvidenceTerminalFacts,
    ) -> ToolEvidenceTerminalDecision {
        if facts.direct_inspection {
            return ToolEvidenceTerminalDecision::CompleteDirectInspectionEmpty;
        }

        if facts.pending_tool_calls == PendingToolCallState::UnresolvedIntent {
            return ToolEvidenceTerminalDecision::IncompletePendingToolCalls;
        }

        if facts.has_tool_results {
            return ToolEvidenceTerminalDecision::ContinueRootSynthesis;
        }

        if facts.pending_tool_calls == PendingToolCallState::ResourceBoundary {
            return ToolEvidenceTerminalDecision::IncompletePendingToolCalls;
        }

        if facts.has_policy_deferred_results {
            return ToolEvidenceTerminalDecision::ContinuePolicyDeferred;
        }

        ToolEvidenceTerminalDecision::Incomplete
    }

    pub(super) fn validate_final_response(
        facts: FinalResponseValidationFacts<'_>,
    ) -> FinalResponseValidationOutcome {
        if facts.attempted_tool_activity {
            return FinalResponseValidationOutcome::Retry(
                FinalResponseViolation::ToolActivityAttempted,
            );
        }

        if facts.response_truncated {
            return FinalResponseValidationOutcome::Retry(
                FinalResponseViolation::TruncatedResponse,
            );
        }

        let Some(text) = facts.response_text else {
            return FinalResponseValidationOutcome::Retry(FinalResponseViolation::EmptyResponse);
        };
        let text = text.trim();
        if text.is_empty() {
            return FinalResponseValidationOutcome::Retry(FinalResponseViolation::EmptyResponse);
        }

        if looks_like_tool_request_text(text) {
            return FinalResponseValidationOutcome::Retry(
                FinalResponseViolation::ToolActivityAttempted,
            );
        }

        if looks_like_progress_only_final_response(text) {
            return FinalResponseValidationOutcome::Retry(
                FinalResponseViolation::ProgressOnlyResponse,
            );
        }

        FinalResponseValidationOutcome::Accept
    }
}

fn looks_like_progress_only_final_response(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    const PREFIXES: &[&str] = &[
        "i'm continuing ",
        "i am continuing ",
        "i’ll continue ",
        "i will continue ",
        "continuing the inspection",
        "continuing to inspect",
        "now i need to ",
        "next i need to ",
        "let me read ",
        "let me inspect ",
        "let me check ",
        "i need to check ",
        "i need to inspect ",
        "i need to read ",
        "drafting the final response:",
        "produce the final answer from observed tool evidence",
    ];
    if PREFIXES.iter().any(|prefix| collapsed.starts_with(prefix)) {
        return true;
    }

    const MARKERS: &[&str] = &[
        " let me read ",
        " let me inspect ",
        " let me check ",
        " now i need to check ",
        " now i need to inspect ",
        " next i need to check ",
        " next i need to inspect ",
        " i still need to ",
    ];
    MARKERS.iter().any(|marker| collapsed.contains(marker))
}

fn looks_like_tool_request_text(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    normalized.starts_with("tool request:")
        || normalized.starts_with("tool call:")
        || normalized.starts_with("function call:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_continuation_accepts_terminal_profiles() {
        let profile = TurnExecutionProfile::DirectInspection(
            crate::loop_engine::direct_inspection::DirectInspectionProfile::ReadLocalPath,
        );

        assert_eq!(
            TurnControlPlane::decide_tool_continuation(ToolContinuationFacts {
                turn_execution_profile: &profile,
                task_phase: None,
                next_tool_scope: None,
                used_mutation_tools: false,
                observation_pressure_saturated: false,
                mutation_tools_available: true,
                artifact_write_pending: false,
            }),
            FinalAnswerDecision::Accept
        );
    }

    #[test]
    fn tool_continuation_does_not_finish_while_gathering() {
        let profile = TurnExecutionProfile::Standard;

        assert_eq!(
            TurnControlPlane::decide_tool_continuation(ToolContinuationFacts {
                turn_execution_profile: &profile,
                task_phase: Some(TaskPhase::Gathering),
                next_tool_scope: None,
                used_mutation_tools: true,
                observation_pressure_saturated: true,
                mutation_tools_available: true,
                artifact_write_pending: false,
            }),
            FinalAnswerDecision::Continue(ContinueReason::GatheringEvidence)
        );
    }

    #[test]
    fn observation_pressure_allows_bounded_synthesis_without_side_effect_requirement() {
        let profile = TurnExecutionProfile::Standard;

        assert_eq!(
            TurnControlPlane::decide_tool_continuation(ToolContinuationFacts {
                turn_execution_profile: &profile,
                task_phase: None,
                next_tool_scope: None,
                used_mutation_tools: false,
                observation_pressure_saturated: true,
                mutation_tools_available: true,
                artifact_write_pending: false,
            }),
            FinalAnswerDecision::Accept
        );
    }

    #[test]
    fn artifact_write_pending_requires_side_effect_before_final_answer() {
        let profile = TurnExecutionProfile::Standard;

        assert_eq!(
            TurnControlPlane::decide_tool_continuation(ToolContinuationFacts {
                turn_execution_profile: &profile,
                task_phase: None,
                next_tool_scope: None,
                used_mutation_tools: false,
                observation_pressure_saturated: false,
                mutation_tools_available: true,
                artifact_write_pending: true,
            }),
            FinalAnswerDecision::Continue(ContinueReason::NoSideEffectProgress)
        );
    }

    #[test]
    fn observation_pressure_requires_mutation_when_artifact_write_is_pending() {
        let profile = TurnExecutionProfile::Standard;

        assert_eq!(
            TurnControlPlane::decide_observation_follow_up(ObservationFollowUpFacts {
                turn_execution_profile: &profile,
                used_observation_tools: true,
                used_mutation_tools: false,
                observation_pressure_saturated: true,
                mutation_tools_available: true,
                artifact_write_pending: true,
            }),
            ObservationFollowUpDecision::RequireMutationOnly
        );
    }

    #[test]
    fn observation_without_pending_artifact_does_not_force_mutation_scope() {
        let profile = TurnExecutionProfile::Standard;

        assert_eq!(
            TurnControlPlane::decide_observation_follow_up(ObservationFollowUpFacts {
                turn_execution_profile: &profile,
                used_observation_tools: true,
                used_mutation_tools: false,
                observation_pressure_saturated: false,
                mutation_tools_available: true,
                artifact_write_pending: false,
            }),
            ObservationFollowUpDecision::Continue
        );
    }

    #[test]
    fn tool_evidence_terminal_commits_observed_evidence_to_root_synthesis() {
        assert_eq!(
            TurnControlPlane::decide_tool_evidence_terminal(ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: false,
                pending_tool_calls: PendingToolCallState::None,
                direct_inspection: false,
            }),
            ToolEvidenceTerminalDecision::ContinueRootSynthesis
        );
    }

    #[test]
    fn tool_evidence_terminal_synthesizes_from_successful_evidence_before_deferred_follow_up() {
        assert_eq!(
            TurnControlPlane::decide_tool_evidence_terminal(ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: true,
                pending_tool_calls: PendingToolCallState::None,
                direct_inspection: false,
            }),
            ToolEvidenceTerminalDecision::ContinueRootSynthesis
        );
    }

    #[test]
    fn tool_evidence_terminal_preserves_policy_deferred_continuation_without_evidence() {
        assert_eq!(
            TurnControlPlane::decide_tool_evidence_terminal(ToolEvidenceTerminalFacts {
                has_tool_results: false,
                has_policy_deferred_results: true,
                pending_tool_calls: PendingToolCallState::None,
                direct_inspection: false,
            }),
            ToolEvidenceTerminalDecision::ContinuePolicyDeferred
        );
    }

    #[test]
    fn tool_evidence_terminal_keeps_direct_inspection_terminal() {
        assert_eq!(
            TurnControlPlane::decide_tool_evidence_terminal(ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: false,
                pending_tool_calls: PendingToolCallState::None,
                direct_inspection: true,
            }),
            ToolEvidenceTerminalDecision::CompleteDirectInspectionEmpty
        );
    }

    #[test]
    fn tool_evidence_terminal_does_not_synthesize_over_pending_tool_intent() {
        assert_eq!(
            TurnControlPlane::decide_tool_evidence_terminal(ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: false,
                pending_tool_calls: PendingToolCallState::UnresolvedIntent,
                direct_inspection: false,
            }),
            ToolEvidenceTerminalDecision::IncompletePendingToolCalls
        );
    }

    #[test]
    fn tool_evidence_terminal_allows_budget_boundary_synthesis_from_observed_evidence() {
        assert_eq!(
            TurnControlPlane::decide_tool_evidence_terminal(ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: false,
                pending_tool_calls: PendingToolCallState::ResourceBoundary,
                direct_inspection: false,
            }),
            ToolEvidenceTerminalDecision::ContinueRootSynthesis
        );
    }

    #[test]
    fn final_response_validation_rejects_tool_activity_in_finalize_phase() {
        assert_eq!(
            TurnControlPlane::validate_final_response(FinalResponseValidationFacts {
                attempted_tool_activity: true,
                response_truncated: false,
                response_text: None,
            }),
            FinalResponseValidationOutcome::Retry(FinalResponseViolation::ToolActivityAttempted)
        );
    }

    #[test]
    fn final_response_validation_rejects_truncated_text() {
        assert_eq!(
            TurnControlPlane::validate_final_response(FinalResponseValidationFacts {
                attempted_tool_activity: false,
                response_truncated: true,
                response_text: Some("Partial final answer"),
            }),
            FinalResponseValidationOutcome::Retry(FinalResponseViolation::TruncatedResponse)
        );
    }

    #[test]
    fn final_response_validation_rejects_tool_shaped_text() {
        assert_eq!(
            TurnControlPlane::validate_final_response(FinalResponseValidationFacts {
                attempted_tool_activity: false,
                response_truncated: false,
                response_text: Some(
                    "Tool request: run_command {\"command\":\"grep -n reduceStream ChatViewModel.swift\"}",
                ),
            }),
            FinalResponseValidationOutcome::Retry(FinalResponseViolation::ToolActivityAttempted)
        );
    }

    #[test]
    fn final_response_validation_rejects_progress_only_text() {
        assert_eq!(
            TurnControlPlane::validate_final_response(FinalResponseValidationFacts {
                attempted_tool_activity: false,
                response_truncated: false,
                response_text: Some(
                    "I'm continuing the inspection. I've already seen the view layer. Let me read the model layer first.",
                ),
            }),
            FinalResponseValidationOutcome::Retry(FinalResponseViolation::ProgressOnlyResponse)
        );
    }

    #[test]
    fn final_response_validation_accepts_answer_text() {
        assert_eq!(
            TurnControlPlane::validate_final_response(FinalResponseValidationFacts {
                attempted_tool_activity: false,
                response_truncated: false,
                response_text: Some("The loop is failing because finalize mode still executes tool decisions. The fix is to enforce final-only state in the control plane."),
            }),
            FinalResponseValidationOutcome::Accept
        );
    }
}
