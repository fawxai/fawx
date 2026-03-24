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

/// Rough cost estimate in USD using approximate Sonnet-tier per-token pricing.
fn estimate_cost_usd(input_tokens: u64, output_tokens: u64) -> f64 {
    // Conservative estimate: ~$3/M input, ~$15/M output (Claude Sonnet-tier)
    let input_cost = (input_tokens as f64) * 3.0 / 1_000_000.0;
    let output_cost = (output_tokens as f64) * 15.0 / 1_000_000.0;
    ((input_cost + output_cost) * 100.0).round() / 100.0
}

// GET /v1/usage
pub async fn handle_usage(State(state): State<HttpState>) -> Json<UsageResponse> {
    let (input_tokens, output_tokens) = session_token_usage(&state).await;
    let total_tokens = input_tokens.saturating_add(output_tokens);
    let estimated_cost = estimate_cost_usd(input_tokens, output_tokens);

    let model = active_model_name(&state).await;

    Json(UsageResponse {
        session: SessionUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            estimated_cost_usd: estimated_cost,
        },
        // Today's usage = session usage for now (no persistent tracking yet)
        today: PeriodUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            estimated_cost_usd: estimated_cost,
        },
        providers: provider_usage(input_tokens, output_tokens, estimated_cost, &model),
    })
}

async fn session_token_usage(state: &HttpState) -> (u64, u64) {
    state.shared.read().await.token_usage
}

async fn active_model_name(state: &HttpState) -> String {
    state.shared.read().await.active_model
}

fn provider_usage(
    input_tokens: u64,
    output_tokens: u64,
    estimated_cost: f64,
    model: &str,
) -> Vec<ProviderUsage> {
    if input_tokens == 0 && output_tokens == 0 {
        return vec![];
    }
    let provider = model.split('/').next().unwrap_or("unknown").to_string();
    vec![ProviderUsage {
        provider,
        model: model.to_string(),
        input_tokens,
        output_tokens,
        estimated_cost_usd: estimated_cost,
    }]
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

    #[test]
    fn estimate_cost_produces_reasonable_values() {
        assert_eq!(estimate_cost_usd(0, 0), 0.0);
        assert_eq!(estimate_cost_usd(1_000_000, 0), 3.0);
        assert_eq!(estimate_cost_usd(0, 1_000_000), 15.0);
    }

    #[test]
    fn provider_usage_empty_when_no_tokens() {
        let providers = provider_usage(0, 0, 0.0, "anthropic/claude-sonnet");
        assert!(providers.is_empty());
    }

    #[test]
    fn provider_usage_extracts_provider_from_model() {
        let providers = provider_usage(100, 50, 0.01, "anthropic/claude-sonnet");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider, "anthropic");
    }
}
