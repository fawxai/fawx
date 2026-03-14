use axum::http::StatusCode;
use axum::Json;

use crate::types::ErrorBody;

pub(crate) type HandlerResult<T> = Result<T, (StatusCode, Json<ErrorBody>)>;

pub mod config;
pub mod devices;
pub mod fleet;
pub mod health;
pub mod message;
pub mod pairing;
pub(crate) mod sessions;
pub(crate) mod settings;
pub mod webhook;
