use axum::http::StatusCode;
use axum::Json;

use crate::types::ErrorBody;

pub(crate) type HandlerResult<T> = Result<T, (StatusCode, Json<ErrorBody>)>;

pub mod auth;
pub mod config;
pub mod cron;
pub mod devices;
pub mod errors;
pub mod fleet;
pub mod health;
pub mod launchagent;
pub mod message;
pub mod pairing;
pub mod permissions;
pub mod phase4;
pub mod proposals;
pub(crate) mod sessions;
pub(crate) mod settings;
pub mod usage;
pub mod webhook;
