use crate::types::ErrorBody;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::HandlerResult;

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentsListResponse {
    pub experiments: Vec<ExperimentSummary>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateExperimentRequest {
    /// Experiment name. Currently unused (stub).
    #[allow(dead_code)]
    pub name: String,
    /// Experiment kind. Currently unused (stub).
    #[allow(dead_code)]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentDetailResponse {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub progress: Option<ExperimentProgress>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentProgress {
    pub completed_steps: usize,
    pub total_steps: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentResultsResponse {
    pub id: String,
    pub status: String,
    pub leaders: Vec<ExperimentLeader>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentLeader {
    pub chain_id: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StopExperimentResponse {
    pub id: String,
    pub stopping: bool,
}

// GET /v1/experiments
pub async fn handle_list_experiments() -> Json<ExperimentsListResponse> {
    Json(ExperimentsListResponse {
        experiments: vec![],
        total: 0,
    })
}

// POST /v1/experiments
pub async fn handle_create_experiment(
    Json(_request): Json<CreateExperimentRequest>,
) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ErrorBody {
            error: "Experiment creation not yet available. Use CLI: fawx improve".to_string(),
        }),
    )
}

// GET /v1/experiments/{id}
pub async fn handle_get_experiment(
    Path(id): Path<String>,
) -> HandlerResult<Json<ExperimentDetailResponse>> {
    Err(experiment_not_found(&id))
}

// GET /v1/experiments/{id}/results
pub async fn handle_get_experiment_results(
    Path(id): Path<String>,
) -> HandlerResult<Json<ExperimentResultsResponse>> {
    Err(experiment_not_found(&id))
}

// POST /v1/experiments/{id}/stop
pub async fn handle_stop_experiment(
    Path(id): Path<String>,
) -> HandlerResult<Json<StopExperimentResponse>> {
    Err(experiment_not_found(&id))
}

fn experiment_not_found(id: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("Experiment '{id}' not found"),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_response_serializes() {
        let r = ExperimentsListResponse {
            experiments: vec![],
            total: 0,
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["total"], 0);
    }

    #[test]
    fn create_request_deserializes() {
        let json = r#"{"name":"test","kind":"proof_of_fitness"}"#;
        let _: CreateExperimentRequest = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn detail_response_serializes() {
        let r = ExperimentDetailResponse {
            id: "exp1".into(),
            name: "Test".into(),
            kind: "proof_of_fitness".into(),
            status: "running".into(),
            created_at: 1_700_000_000,
            started_at: Some(1_700_000_060),
            completed_at: None,
            progress: Some(ExperimentProgress {
                completed_steps: 5,
                total_steps: 10,
            }),
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["progress"]["completed_steps"], 5);
    }

    #[test]
    fn results_response_serializes() {
        let r = ExperimentResultsResponse {
            id: "exp1".into(),
            status: "running".into(),
            leaders: vec![ExperimentLeader {
                chain_id: "chain-a".into(),
                score: 91.2,
            }],
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["leaders"][0]["score"], 91.2);
    }

    #[test]
    fn stop_response_serializes() {
        let r = StopExperimentResponse {
            id: "exp1".into(),
            stopping: true,
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["stopping"], true);
    }

    #[tokio::test]
    async fn list_returns_empty() {
        let Json(response) = handle_list_experiments().await;
        assert!(response.experiments.is_empty());
    }

    #[tokio::test]
    async fn create_returns_not_implemented() {
        let (status, _) = handle_create_experiment(Json(CreateExperimentRequest {
            name: "test".into(),
            kind: "proof_of_fitness".into(),
        }))
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn get_returns_not_found() {
        let err = handle_get_experiment(Path("missing".into()))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }
}
