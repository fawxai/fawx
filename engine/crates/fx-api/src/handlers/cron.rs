use crate::handlers::message::internal_error;
use crate::handlers::HandlerResult;
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use fx_cron::{
    next_run_time, now_ms, trigger_job, validate_schedule, CronError, CronJob, CronStore,
    JobPayload, JobRun, Schedule,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

const TZ_UNSUPPORTED: &str = "timezone support not yet implemented";

#[derive(Debug, Deserialize)]
pub struct CreateJobRequest {
    pub name: Option<String>,
    pub schedule: Schedule,
    pub payload: JobPayload,
}

#[derive(Debug, Deserialize)]
pub struct UpdateJobRequest {
    pub name: Option<String>,
    pub schedule: Schedule,
    pub payload: JobPayload,
    pub enabled: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct JobListResponse {
    pub jobs: Vec<CronJob>,
    pub total: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct JobRunsResponse {
    pub runs: Vec<JobRun>,
    pub total: usize,
}

pub async fn handle_list_jobs(State(state): State<HttpState>) -> HandlerResult<Response> {
    let store = require_cron_store(&state)?;
    let jobs = store
        .lock()
        .await
        .list_jobs()
        .map_err(cron_internal_error)?;
    let total = jobs.len();
    Ok(Json(JobListResponse { jobs, total }).into_response())
}

pub async fn handle_create_job(
    State(state): State<HttpState>,
    Json(request): Json<CreateJobRequest>,
) -> HandlerResult<Response> {
    reject_timezone(&request.schedule)?;
    validate_schedule(&request.schedule).map_err(bad_request)?;
    let store = require_cron_store(&state)?;
    let now_ms = now_ms();
    let job = CronJob {
        id: Uuid::new_v4(),
        name: request.name,
        next_run_at: next_run_time(&request.schedule, now_ms),
        schedule: request.schedule,
        payload: request.payload,
        enabled: true,
        created_at: now_ms,
        updated_at: now_ms,
        last_run_at: None,
        run_count: 0,
    };
    store
        .lock()
        .await
        .upsert_job(&job)
        .map_err(cron_internal_error)?;
    Ok((StatusCode::CREATED, Json(job)).into_response())
}

pub async fn handle_get_job(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<Response> {
    let store = require_cron_store(&state)?;
    let job = store
        .lock()
        .await
        .get_job(id)
        .map_err(cron_internal_error)?;
    match job {
        Some(job) => Ok(Json(job).into_response()),
        None => Err(not_found("cron job not found")),
    }
}

pub async fn handle_update_job(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateJobRequest>,
) -> HandlerResult<Response> {
    reject_timezone(&request.schedule)?;
    validate_schedule(&request.schedule).map_err(bad_request)?;
    let store = require_cron_store(&state)?;
    let now_ms = now_ms();
    let store = store.lock().await;
    let Some(mut existing) = store.get_job(id).map_err(cron_internal_error)? else {
        return Err(not_found("cron job not found"));
    };
    existing.name = request.name;
    existing.schedule = request.schedule;
    existing.payload = request.payload;
    existing.enabled = request.enabled;
    existing.updated_at = now_ms;
    existing.next_run_at = if existing.enabled {
        next_run_time(&existing.schedule, now_ms)
    } else {
        None
    };
    store.upsert_job(&existing).map_err(cron_internal_error)?;
    Ok(Json(existing).into_response())
}

pub async fn handle_delete_job(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<Response> {
    let store = require_cron_store(&state)?;
    let deleted = store
        .lock()
        .await
        .delete_job(id)
        .map_err(cron_internal_error)?;
    if deleted {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    Err(not_found("cron job not found"))
}

pub async fn handle_trigger_job(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<Response> {
    let store = require_cron_store(&state)?;
    let bus = load_session_bus(&state).await?;
    let run = trigger_job(&store, &bus, id)
        .await
        .map_err(cron_internal_error)?
        .ok_or_else(|| not_found("cron job not found"))?;
    Ok(Json(run).into_response())
}

pub async fn handle_list_runs(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<Response> {
    let store = require_cron_store(&state)?;
    let runs = store
        .lock()
        .await
        .list_runs(id)
        .map_err(cron_internal_error)?;
    let total = runs.len();
    Ok(Json(JobRunsResponse { runs, total }).into_response())
}

fn require_cron_store(state: &HttpState) -> HandlerResult<Arc<Mutex<CronStore>>> {
    state
        .cron_store
        .clone()
        .ok_or_else(|| service_unavailable("cron store unavailable"))
}

async fn load_session_bus(state: &HttpState) -> HandlerResult<fx_bus::SessionBus> {
    let app = state.app.lock().await;
    app.session_bus()
        .cloned()
        .ok_or_else(|| service_unavailable("session bus unavailable"))
}

fn reject_timezone(schedule: &Schedule) -> HandlerResult<()> {
    if matches!(schedule, Schedule::Cron { tz: Some(_), .. }) {
        return Err(bad_request_message(TZ_UNSUPPORTED));
    }
    Ok(())
}

fn bad_request(error: CronError) -> (StatusCode, Json<ErrorBody>) {
    bad_request_message(&error.to_string())
}

fn bad_request_message(message: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: message.to_string(),
        }),
    )
}

fn cron_internal_error(error: CronError) -> (StatusCode, Json<ErrorBody>) {
    internal_error(anyhow::Error::new(error))
}

fn not_found(message: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: message.to_string(),
        }),
    )
}

fn service_unavailable(message: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorBody {
            error: message.to_string(),
        }),
    )
}
