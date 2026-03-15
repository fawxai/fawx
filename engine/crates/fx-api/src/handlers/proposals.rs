use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::HandlerResult;

/// Risk tier for proposal classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalTier {
    Standard,  // routine operations — green
    Elevated,  // system commands, config changes — amber
    Sensitive, // auth/credentials, TIER2 paths — red
}

/// A pending proposal awaiting user approval
#[derive(Debug, Clone, Serialize)]
pub struct PendingProposal {
    pub id: String,
    pub tier: ProposalTier,
    pub action: String,       // "write_file", "execute_command", etc.
    pub target: String,       // full path or resource
    pub agent_reason: String, // agent's stated reason (display separately from action)
    pub diff: Option<String>, // unified diff if file write
    pub created_at: u64,      // unix timestamp
}

/// Response for listing pending proposals
#[derive(Debug, Clone, Serialize)]
pub struct PendingProposalsResponse {
    pub proposals: Vec<PendingProposal>,
    pub total: usize,
}

/// Request to approve or deny a proposal
#[derive(Debug, Deserialize)]
pub struct ProposalDecisionRequest {
    pub approved: bool,
}

/// Response after approving/denying
#[derive(Debug, Clone, Serialize)]
pub struct ProposalDecisionResponse {
    pub id: String,
    pub approved: bool,
    pub message: String,
}

/// Response for proposal history
#[derive(Debug, Clone, Serialize)]
pub struct ProposalHistoryEntry {
    pub id: String,
    pub tier: ProposalTier,
    pub action: String,
    pub target: String,
    pub agent_reason: String,
    pub approved: bool,
    pub decided_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProposalHistoryResponse {
    pub entries: Vec<ProposalHistoryEntry>,
    pub total: usize,
}

// GET /v1/proposals/pending — list proposals awaiting approval
pub async fn handle_list_pending(
    State(_state): State<HttpState>,
) -> Json<PendingProposalsResponse> {
    // TODO: wire to ProposalGateState to read pending proposals
    Json(PendingProposalsResponse {
        proposals: vec![],
        total: 0,
    })
}

// POST /v1/proposals/:id/decide — approve or deny a proposal
pub async fn handle_decide(
    State(_state): State<HttpState>,
    Path(id): Path<String>,
    Json(request): Json<ProposalDecisionRequest>,
) -> HandlerResult<Json<ProposalDecisionResponse>> {
    // TODO: wire to ProposalGateState to approve/deny
    let action = if request.approved {
        "approved"
    } else {
        "denied"
    };
    Ok(Json(ProposalDecisionResponse {
        id,
        approved: request.approved,
        message: format!("Proposal {action}."),
    }))
}

// GET /v1/proposals/:id/diff — get the full diff for a proposal
pub async fn handle_get_diff(
    State(_state): State<HttpState>,
    Path(id): Path<String>,
) -> HandlerResult<Json<PendingProposal>> {
    // TODO: wire to ProposalGateState to read proposal details
    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("Proposal {id} not found"),
        }),
    ))
}

// GET /v1/proposals/history — list past decisions
pub async fn handle_history(State(_state): State<HttpState>) -> Json<ProposalHistoryResponse> {
    // TODO: wire to ProposalGateState history
    Json(ProposalHistoryResponse {
        entries: vec![],
        total: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_response_serializes() {
        let r = PendingProposalsResponse {
            proposals: vec![],
            total: 0,
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["total"], 0);
    }

    #[test]
    fn decision_response_serializes() {
        let r = ProposalDecisionResponse {
            id: "p1".into(),
            approved: true,
            message: "ok".into(),
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["approved"], true);
    }

    #[test]
    fn history_response_serializes() {
        let r = ProposalHistoryResponse {
            entries: vec![],
            total: 0,
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["total"], 0);
    }

    #[test]
    fn proposal_tier_serializes_standard() {
        assert_eq!(
            serde_json::to_value(ProposalTier::Standard).unwrap(),
            "standard"
        );
    }

    #[test]
    fn proposal_tier_serializes_elevated() {
        assert_eq!(
            serde_json::to_value(ProposalTier::Elevated).unwrap(),
            "elevated"
        );
    }

    #[test]
    fn proposal_tier_serializes_as_snake_case() {
        let json = serde_json::to_value(ProposalTier::Sensitive).unwrap();
        assert_eq!(json, "sensitive");
    }

    #[test]
    fn pending_proposal_serializes_full() {
        let proposal = PendingProposal {
            id: "p1".into(),
            tier: ProposalTier::Elevated,
            action: "write_file".into(),
            target: "/etc/config".into(),
            agent_reason: "Need to update config".into(),
            diff: Some("+new line".into()),
            created_at: 1_700_000_000,
        };
        let json = serde_json::to_value(proposal).unwrap();
        assert_eq!(json["tier"], "elevated");
        assert_eq!(json["action"], "write_file");
        assert!(json["diff"].is_string());
    }

    #[test]
    fn decision_request_deserializes() {
        let json = r#"{"approved": true}"#;
        let request: ProposalDecisionRequest = serde_json::from_str(json).unwrap();
        assert!(request.approved);
    }

    #[test]
    fn history_entry_serializes_agent_reason() {
        let entry = ProposalHistoryEntry {
            id: "p1".into(),
            tier: ProposalTier::Elevated,
            action: "write_file".into(),
            target: "/etc/config".into(),
            agent_reason: "Need to update config".into(),
            approved: true,
            decided_at: 1_700_000_001,
        };
        let json = serde_json::to_value(entry).unwrap();
        assert_eq!(json["agent_reason"], "Need to update config");
    }
}
