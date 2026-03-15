use crate::state::HttpState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct UsageResponse {
    pub session: SessionUsage,
    pub today: PeriodUsage,
    pub providers: Vec<ProviderUsage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeriodUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderUsage {
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_usd: f64,
}

// GET /v1/usage
pub async fn handle_usage(State(_state): State<HttpState>) -> Json<UsageResponse> {
    // TODO(E-2): wire to actual token tracking from LoopEngine/BudgetTracker.
    Json(UsageResponse {
        session: SessionUsage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        },
        today: PeriodUsage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        },
        providers: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_response_serializes() {
        let response = UsageResponse {
            session: SessionUsage {
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
                estimated_cost_usd: 0.01,
            },
            today: PeriodUsage {
                input_tokens: 1000,
                output_tokens: 500,
                total_tokens: 1500,
                estimated_cost_usd: 0.10,
            },
            providers: vec![ProviderUsage {
                provider: "anthropic".into(),
                model: "claude-opus-4-6".into(),
                input_tokens: 1000,
                output_tokens: 500,
                estimated_cost_usd: 0.10,
            }],
        };

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["session"]["total_tokens"], 150);
        assert_eq!(json["providers"][0]["provider"], "anthropic");
    }
}
