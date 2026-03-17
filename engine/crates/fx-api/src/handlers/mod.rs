use axum::http::StatusCode;
use axum::Json;

use crate::types::ErrorBody;

pub(crate) type HandlerResult<T> = Result<T, (StatusCode, Json<ErrorBody>)>;

pub mod auth;
pub mod config;
pub mod cron;
pub mod devices;
pub mod errors;
pub mod experiments;
pub mod fleet;
pub mod fleet_dashboard;
pub mod git;
pub mod health;
pub mod launchagent;
pub mod marketplace;
pub mod message;
pub mod oauth;
pub mod pairing;
pub mod permission_prompts;
pub mod permissions;
pub mod phase4;
pub mod proposals;
pub mod ripcord;
pub(crate) mod sessions;
pub(crate) mod settings;
pub mod synthesis;
pub mod telemetry;
pub mod usage;
pub mod webhook;
