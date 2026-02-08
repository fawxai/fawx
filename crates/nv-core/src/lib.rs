//! Core types, configuration, event bus, and error handling for Nova.
//!
//! This crate provides the foundational types and utilities used across
//! all Nova crates. It includes:
//! - Configuration management
//! - Inter-crate messaging types
//! - Event bus for asynchronous communication
//! - Error taxonomy
//! - Shared types (intents, action plans, etc.)

pub mod config;
pub mod error;
pub mod event;
pub mod message;
pub mod types;

pub use config::Config;
pub use error::{CoreError, Result};
pub use event::EventBus;
