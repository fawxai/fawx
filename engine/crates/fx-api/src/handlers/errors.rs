use crate::state::HttpState;
use crate::types::RecentErrorsResponse;
use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

const DEFAULT_ERROR_LIMIT: usize = 20;

#[derive(Debug, Deserialize)]
pub struct RecentErrorsQuery {
    pub limit: Option<usize>,
}

pub async fn handle_recent_errors(
    State(state): State<HttpState>,
    Query(query): Query<RecentErrorsQuery>,
) -> Json<RecentErrorsResponse> {
    let app = state.app.lock().await;
    Json(RecentErrorsResponse {
        errors: app.recent_errors(query.limit.unwrap_or(DEFAULT_ERROR_LIMIT)),
    })
}
