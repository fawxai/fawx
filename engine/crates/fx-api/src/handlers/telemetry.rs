use crate::handlers::HandlerResult;
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use fx_telemetry::{SignalCategory, TelemetryConsent, TelemetrySignal};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct ConsentResponse {
    pub enabled: bool,
    pub categories: HashMap<String, CategoryInfo>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct CategoryInfo {
    pub enabled: bool,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct ConsentPatch {
    pub enabled: Option<bool>,
    pub categories: Option<HashMap<String, bool>>,
}

#[derive(Debug, Serialize)]
pub struct SignalsResponse {
    pub count: usize,
    pub signals: Vec<TelemetrySignal>,
}

pub async fn handle_get_consent(State(state): State<HttpState>) -> Json<ConsentResponse> {
    Json(consent_to_response(&state.telemetry.consent()))
}

pub async fn handle_patch_consent(
    State(state): State<HttpState>,
    Json(patch): Json<ConsentPatch>,
) -> HandlerResult<Json<ConsentResponse>> {
    let mut consent = state.telemetry.consent();
    let changed = patch.enabled.is_some() || patch.categories.is_some();
    apply_enabled_patch(&mut consent, patch.enabled);
    apply_category_patch(&mut consent, patch.categories);
    if changed {
        consent.updated_at = Utc::now();
        state
            .telemetry
            .update_consent(consent.clone())
            .map_err(internal_error)?;
    }
    Ok(Json(consent_to_response(&consent)))
}

pub async fn handle_get_signals(State(state): State<HttpState>) -> Json<SignalsResponse> {
    let signals = state.telemetry.drain();
    Json(SignalsResponse {
        count: signals.len(),
        signals,
    })
}

pub async fn handle_delete_signals(State(state): State<HttpState>) -> StatusCode {
    let _ = state.telemetry.drain();
    StatusCode::NO_CONTENT
}

fn apply_enabled_patch(consent: &mut TelemetryConsent, enabled: Option<bool>) {
    match enabled {
        Some(true) => consent.enabled = true,
        Some(false) => consent.disable_all(),
        None => {}
    }
}

fn apply_category_patch(consent: &mut TelemetryConsent, categories: Option<HashMap<String, bool>>) {
    let Some(categories) = categories else {
        return;
    };

    for (name, enabled) in categories {
        let Some(category) = parse_category(&name) else {
            continue;
        };
        if enabled {
            consent.enable_category(category);
        } else {
            consent.disable_category(category);
        }
    }
}

fn consent_to_response(consent: &TelemetryConsent) -> ConsentResponse {
    let categories = SignalCategory::all()
        .into_iter()
        .map(|category| {
            let name = category.to_string();
            let info = CategoryInfo {
                enabled: consent.is_category_enabled(&category),
                description: category.description().to_owned(),
            };
            (name, info)
        })
        .collect();

    ConsentResponse {
        enabled: consent.enabled,
        categories,
        updated_at: consent.updated_at.to_rfc3339(),
    }
}

fn parse_category(name: &str) -> Option<SignalCategory> {
    SignalCategory::all()
        .into_iter()
        .find(|category| category.to_string() == name)
}

fn internal_error(error: impl ToString) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}
