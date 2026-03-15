use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::fs;

/// Local mirror of fx_propose::ProposalSidecar for deserialization.
/// Avoids adding fx-propose as a dependency of fx-api.
#[derive(Debug, Clone, Deserialize)]
struct ProposalSidecar {
    #[allow(dead_code)]
    pub version: u8,
    pub timestamp: u64,
    pub title: String,
    pub description: String,
    pub target_path: String,
    pub proposed_content: String,
    pub risk: String,
    #[allow(dead_code)]
    pub file_hash_at_creation: Option<String>,
}

use super::HandlerResult;

/// Risk tier for proposal classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalTier {
    Standard,
    Elevated,
    Sensitive,
}

/// A pending proposal awaiting user approval.
#[derive(Debug, Clone, Serialize)]
pub struct PendingProposal {
    pub id: String,
    pub tier: ProposalTier,
    pub action: String,
    pub target: String,
    pub agent_reason: String,
    pub diff: Option<String>,
    pub created_at: u64,
}

/// Response for listing pending proposals.
#[derive(Debug, Clone, Serialize)]
pub struct PendingProposalsResponse {
    pub proposals: Vec<PendingProposal>,
    pub total: usize,
}

/// Request to approve or deny a proposal.
#[derive(Debug, Deserialize)]
pub struct ProposalDecisionRequest {
    pub approved: bool,
}

/// Response after approving/denying.
#[derive(Debug, Clone, Serialize)]
pub struct ProposalDecisionResponse {
    pub id: String,
    pub approved: bool,
    pub message: String,
}

/// Response for proposal history.
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
pub async fn handle_list_pending(State(state): State<HttpState>) -> Json<PendingProposalsResponse> {
    let proposals_dir = state.data_dir.join("proposals");
    let proposals = read_pending_proposals(&proposals_dir);
    let total = proposals.len();
    Json(PendingProposalsResponse { proposals, total })
}

// POST /v1/proposals/:id/decide — approve or deny a proposal
pub async fn handle_decide(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Json(request): Json<ProposalDecisionRequest>,
) -> HandlerResult<Json<ProposalDecisionResponse>> {
    let proposals_dir = state.data_dir.join("proposals");
    let sidecar_path =
        find_sidecar_by_id(&proposals_dir, &id).ok_or_else(|| proposal_not_found(&id))?;

    if request.approved {
        // Move to approved directory
        let approved_dir = proposals_dir.join("approved");
        move_proposal_files(&sidecar_path, &approved_dir);
    } else {
        // Move to rejected directory
        let rejected_dir = proposals_dir.join("rejected");
        move_proposal_files(&sidecar_path, &rejected_dir);
    }

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
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> HandlerResult<Json<PendingProposal>> {
    let proposals_dir = state.data_dir.join("proposals");
    let sidecar_path =
        find_sidecar_by_id(&proposals_dir, &id).ok_or_else(|| proposal_not_found(&id))?;
    let sidecar = read_sidecar(&sidecar_path).ok_or_else(|| proposal_not_found(&id))?;
    Ok(Json(sidecar_to_proposal(&sidecar)))
}

// GET /v1/proposals/history — list past decisions
pub async fn handle_history(State(state): State<HttpState>) -> Json<ProposalHistoryResponse> {
    let proposals_dir = state.data_dir.join("proposals");
    let mut entries = Vec::new();
    for (subdir, approved) in [("approved", true), ("rejected", false)] {
        let dir = proposals_dir.join(subdir);
        for sidecar in read_sidecars_from_dir(&dir) {
            entries.push(ProposalHistoryEntry {
                id: proposal_id_from_sidecar(&sidecar),
                tier: classify_risk(&sidecar.risk),
                action: "write_file".to_string(),
                target: sidecar.target_path.clone(),
                agent_reason: sidecar.description.clone(),
                approved,
                decided_at: sidecar.timestamp,
            });
        }
    }
    entries.sort_by(|a, b| b.decided_at.cmp(&a.decided_at));
    let total = entries.len();
    Json(ProposalHistoryResponse { entries, total })
}

fn read_pending_proposals(proposals_dir: &std::path::Path) -> Vec<PendingProposal> {
    read_sidecars_from_dir(proposals_dir)
        .into_iter()
        .map(|s| sidecar_to_proposal(&s))
        .collect()
}

fn sidecar_to_proposal(sidecar: &ProposalSidecar) -> PendingProposal {
    PendingProposal {
        id: proposal_id_from_sidecar(sidecar),
        tier: classify_risk(&sidecar.risk),
        action: "write_file".to_string(),
        target: sidecar.target_path.clone(),
        agent_reason: sidecar.description.clone(),
        diff: Some(sidecar.proposed_content.clone()),
        created_at: sidecar.timestamp,
    }
}

fn proposal_id_from_sidecar(sidecar: &ProposalSidecar) -> String {
    let sanitized = sidecar
        .title
        .chars()
        .take(30)
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>();
    format!("{}_{sanitized}", sidecar.timestamp)
}

fn classify_risk(risk: &str) -> ProposalTier {
    match risk.to_lowercase().as_str() {
        "low" | "standard" => ProposalTier::Standard,
        "medium" | "elevated" => ProposalTier::Elevated,
        "high" | "sensitive" | "critical" => ProposalTier::Sensitive,
        _ => ProposalTier::Standard,
    }
}

fn read_sidecars_from_dir(dir: &std::path::Path) -> Vec<ProposalSidecar> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| read_sidecar(&entry.path()))
        .collect()
}

fn read_sidecar(path: &std::path::Path) -> Option<ProposalSidecar> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn find_sidecar_by_id(proposals_dir: &std::path::Path, id: &str) -> Option<std::path::PathBuf> {
    let timestamp_prefix = id.split('_').next().unwrap_or(id);
    let entries = fs::read_dir(proposals_dir).ok()?;
    entries
        .filter_map(|e| e.ok())
        .find(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".json") && name.starts_with(timestamp_prefix)
        })
        .map(|entry| entry.path())
}

fn move_proposal_files(sidecar_path: &std::path::Path, dest_dir: &std::path::Path) {
    let _ = fs::create_dir_all(dest_dir);
    if let Some(filename) = sidecar_path.file_name() {
        let _ = fs::rename(sidecar_path, dest_dir.join(filename));
    }
    // Also move the .md file
    let md_path = sidecar_path.with_extension("md");
    if let Some(filename) = md_path.file_name() {
        let _ = fs::rename(&md_path, dest_dir.join(filename));
    }
}

fn proposal_not_found(id: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("Proposal {id} not found"),
        }),
    )
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
    fn proposal_tier_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_value(ProposalTier::Standard).unwrap(),
            "standard"
        );
        assert_eq!(
            serde_json::to_value(ProposalTier::Elevated).unwrap(),
            "elevated"
        );
        assert_eq!(
            serde_json::to_value(ProposalTier::Sensitive).unwrap(),
            "sensitive"
        );
    }

    #[test]
    fn classify_risk_maps_values() {
        assert_eq!(classify_risk("low"), ProposalTier::Standard);
        assert_eq!(classify_risk("medium"), ProposalTier::Elevated);
        assert_eq!(classify_risk("high"), ProposalTier::Sensitive);
        assert_eq!(classify_risk("critical"), ProposalTier::Sensitive);
        assert_eq!(classify_risk("unknown"), ProposalTier::Standard);
    }

    #[test]
    fn sidecar_to_proposal_maps_fields() {
        let sidecar = ProposalSidecar {
            version: 1,
            timestamp: 1700000000,
            title: "Update config".into(),
            description: "Need to update config".into(),
            target_path: "/etc/config".into(),
            proposed_content: "+new line".into(),
            risk: "medium".into(),
            file_hash_at_creation: Some("abc123".into()),
        };

        let proposal = sidecar_to_proposal(&sidecar);

        assert_eq!(proposal.tier, ProposalTier::Elevated);
        assert_eq!(proposal.target, "/etc/config");
        assert_eq!(proposal.diff, Some("+new line".into()));
    }

    #[test]
    fn read_sidecars_from_nonexistent_dir_returns_empty() {
        let sidecars = read_sidecars_from_dir(std::path::Path::new("/nonexistent"));
        assert!(sidecars.is_empty());
    }

    #[test]
    fn read_sidecars_from_dir_reads_json_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let sidecar = ProposalSidecar {
            version: 1,
            timestamp: 1700000000,
            title: "Test".into(),
            description: "desc".into(),
            target_path: "file.rs".into(),
            proposed_content: "content".into(),
            risk: "low".into(),
            file_hash_at_creation: None,
        };
        let json = serde_json::to_string(&sidecar).unwrap();
        fs::write(temp.path().join("1700000000-test.json"), &json).unwrap();
        // Write a non-json file that should be ignored
        fs::write(temp.path().join("readme.md"), "# Readme").unwrap();

        let sidecars = read_sidecars_from_dir(temp.path());

        assert_eq!(sidecars.len(), 1);
        assert_eq!(sidecars[0].title, "Test");
    }
}
