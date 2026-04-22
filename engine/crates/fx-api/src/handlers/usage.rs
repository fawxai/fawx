use crate::state::HttpState;
use axum::extract::State;
use axum::Json;
use fx_kernel::TokenUsage;
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
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeriodUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderUsage {
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub estimated_cost_usd: f64,
}

const INPUT_USD_PER_MILLION: f64 = 3.0;
const OUTPUT_USD_PER_MILLION: f64 = 15.0;
const CACHE_READ_USD_PER_MILLION: f64 = 0.30;
const CACHE_WRITE_USD_PER_MILLION: f64 = 3.75;

/// Rough cost estimate in USD using approximate Sonnet-tier per-token pricing.
///
/// Cache prices vary by provider, so this remains an estimate. The important
/// contract is that cache hits are not priced like full fresh input tokens.
fn estimate_cost_usd(usage: TokenUsage) -> f64 {
    let uncached_input_tokens = usage.input_tokens.saturating_sub(usage.cached_input_tokens);
    let input_cost = (uncached_input_tokens as f64) * INPUT_USD_PER_MILLION / 1_000_000.0;
    let cache_read_cost =
        (usage.cached_input_tokens as f64) * CACHE_READ_USD_PER_MILLION / 1_000_000.0;
    let cache_write_cost =
        (usage.cache_creation_input_tokens as f64) * CACHE_WRITE_USD_PER_MILLION / 1_000_000.0;
    let output_cost = (usage.output_tokens as f64) * OUTPUT_USD_PER_MILLION / 1_000_000.0;
    ((input_cost + cache_read_cost + cache_write_cost + output_cost) * 100.0).round() / 100.0
}

// GET /v1/usage
pub async fn handle_usage(State(state): State<HttpState>) -> Json<UsageResponse> {
    let usage = session_token_usage(&state).await;
    let total_tokens = usage.total_tokens();
    let estimated_cost = estimate_cost_usd(usage);

    let model = active_model_name(&state).await;

    Json(UsageResponse {
        session: SessionUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            total_tokens,
            estimated_cost_usd: estimated_cost,
        },
        // Today's usage = session usage for now (no persistent tracking yet)
        today: PeriodUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            total_tokens,
            estimated_cost_usd: estimated_cost,
        },
        providers: provider_usage(usage, estimated_cost, &model),
    })
}

async fn session_token_usage(state: &HttpState) -> TokenUsage {
    state.shared.read().await.token_usage
}

async fn active_model_name(state: &HttpState) -> String {
    state.shared.read().await.active_model
}

fn provider_usage(usage: TokenUsage, estimated_cost: f64, model: &str) -> Vec<ProviderUsage> {
    if usage.is_empty() {
        return vec![];
    }
    let provider = model.split('/').next().unwrap_or("unknown").to_string();
    vec![ProviderUsage {
        provider,
        model: model.to_string(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cached_input_tokens: usage.cached_input_tokens,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
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
                cached_input_tokens: 25,
                cache_creation_input_tokens: 10,
                total_tokens: 150,
                estimated_cost_usd: 0.01,
            },
            today: PeriodUsage {
                input_tokens: 1000,
                output_tokens: 500,
                cached_input_tokens: 250,
                cache_creation_input_tokens: 100,
                total_tokens: 1500,
                estimated_cost_usd: 0.10,
            },
            providers: vec![ProviderUsage {
                provider: "anthropic".into(),
                model: "claude-opus-4-6".into(),
                input_tokens: 1000,
                output_tokens: 500,
                cached_input_tokens: 250,
                cache_creation_input_tokens: 100,
                estimated_cost_usd: 0.10,
            }],
        };

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["session"]["total_tokens"], 150);
        assert_eq!(json["session"]["cached_input_tokens"], 25);
        assert_eq!(json["providers"][0]["cache_creation_input_tokens"], 100);
        assert_eq!(json["providers"][0]["provider"], "anthropic");
    }

    #[test]
    fn estimate_cost_produces_reasonable_values() {
        assert_eq!(estimate_cost_usd(TokenUsage::default()), 0.0);
        assert_eq!(
            estimate_cost_usd(TokenUsage {
                input_tokens: 1_000_000,
                ..Default::default()
            }),
            3.0
        );
        assert_eq!(
            estimate_cost_usd(TokenUsage {
                output_tokens: 1_000_000,
                ..Default::default()
            }),
            15.0
        );
    }

    #[test]
    fn estimate_cost_prices_cache_reads_below_fresh_input() {
        let fresh = estimate_cost_usd(TokenUsage {
            input_tokens: 1_000_000,
            ..Default::default()
        });
        let cached = estimate_cost_usd(TokenUsage {
            input_tokens: 1_000_000,
            cached_input_tokens: 1_000_000,
            ..Default::default()
        });
        assert_eq!(fresh, 3.0);
        assert_eq!(cached, 0.30);
    }

    #[test]
    fn provider_usage_empty_when_no_tokens() {
        let providers = provider_usage(TokenUsage::default(), 0.0, "anthropic/claude-sonnet");
        assert!(providers.is_empty());
    }

    #[test]
    fn provider_usage_visible_when_prompt_cache_only() {
        let providers = provider_usage(
            TokenUsage {
                cached_input_tokens: 30,
                cache_creation_input_tokens: 5,
                ..Default::default()
            },
            0.0,
            "anthropic/claude-sonnet",
        );
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].cached_input_tokens, 30);
        assert_eq!(providers[0].cache_creation_input_tokens, 5);
    }

    #[test]
    fn provider_usage_extracts_provider_from_model() {
        let providers = provider_usage(
            TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                cached_input_tokens: 30,
                cache_creation_input_tokens: 5,
            },
            0.01,
            "anthropic/claude-sonnet",
        );
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider, "anthropic");
        assert_eq!(providers[0].cached_input_tokens, 30);
        assert_eq!(providers[0].cache_creation_input_tokens, 5);
    }
}
