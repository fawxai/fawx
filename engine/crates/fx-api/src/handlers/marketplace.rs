use crate::types::ErrorBody;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

const MARKETPLACE_NOT_CONNECTED_MESSAGE: &str = "Marketplace not yet connected";
const MARKETPLACE_UNAVAILABLE_MESSAGE: &str =
    "Marketplace not yet available. Install skills via CLI: fawx skills install <name>";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceSkillSummary {
    pub name: String,
    pub title: String,
    pub description: String,
    pub publisher: String,
    pub signed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSearchResponse {
    pub query: String,
    pub skills: Vec<MarketplaceSkillSummary>,
    pub total: usize,
    pub marketplace_available: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallSkillRequest {
    /// Skill name to install. Currently unused (stub), but validated by
    /// deserialization to ensure the request shape is correct.
    #[allow(dead_code)]
    pub name: String,
}

pub async fn handle_search_skills(Query(params): Query<SearchQuery>) -> Json<SkillSearchResponse> {
    Json(SkillSearchResponse {
        query: params.q,
        skills: vec![],
        total: 0,
        marketplace_available: false,
        message: MARKETPLACE_NOT_CONNECTED_MESSAGE.to_string(),
    })
}

pub async fn handle_install_skill(
    Json(_request): Json<InstallSkillRequest>,
) -> (StatusCode, Json<ErrorBody>) {
    marketplace_unavailable()
}

pub async fn handle_remove_skill(Path(name): Path<String>) -> (StatusCode, Json<ErrorBody>) {
    skill_not_found(name)
}

fn marketplace_unavailable() -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorBody {
            error: MARKETPLACE_UNAVAILABLE_MESSAGE.to_string(),
        }),
    )
}

fn skill_not_found(name: String) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("Skill '{name}' not found"),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_response_serializes() {
        let response = SkillSearchResponse {
            query: "portfolio".into(),
            skills: vec![],
            total: 0,
            marketplace_available: false,
            message: MARKETPLACE_NOT_CONNECTED_MESSAGE.into(),
        };

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["query"], "portfolio");
        assert_eq!(json["total"], 0);
        assert_eq!(json["marketplace_available"], false);
    }

    #[test]
    fn skill_summary_serializes() {
        let skill = MarketplaceSkillSummary {
            name: "portfolio-tracker".into(),
            title: "Portfolio Tracker".into(),
            description: "Track holdings and prices.".into(),
            publisher: "fawx-ai".into(),
            signed: true,
        };

        let json = serde_json::to_value(skill).unwrap();

        assert_eq!(json["name"], "portfolio-tracker");
        assert_eq!(json["signed"], true);
    }

    #[test]
    fn install_request_deserializes() {
        let json = r#"{"name":"portfolio-tracker"}"#;
        let request: InstallSkillRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.name, "portfolio-tracker");
    }

    #[tokio::test]
    async fn search_handler_returns_empty_results() {
        let params = SearchQuery {
            q: "portfolio".into(),
        };
        let response = handle_search_skills(Query(params)).await;

        assert_eq!(response.0.query, "portfolio");
        assert!(response.0.skills.is_empty());
        assert!(!response.0.marketplace_available);
    }

    #[tokio::test]
    async fn install_handler_returns_service_unavailable() {
        let request = InstallSkillRequest {
            name: "portfolio-tracker".into(),
        };
        let (status, body) = handle_install_skill(Json(request)).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.0.error, MARKETPLACE_UNAVAILABLE_MESSAGE);
    }

    #[tokio::test]
    async fn remove_handler_returns_not_found() {
        let (status, body) = handle_remove_skill(Path(String::from("portfolio-tracker"))).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.0.error, "Skill 'portfolio-tracker' not found");
    }
}
