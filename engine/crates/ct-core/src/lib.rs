//! Core types, configuration, event bus, and error handling for Citros.
//!
//! This crate provides the foundational types and utilities used across
//! all Citros crates. It includes:
//! - Configuration management
//! - Inter-crate messaging types
//! - Event bus for asynchronous communication
//! - Error taxonomy
//! - Shared types (intents, action plans, etc.)
//! - [`PhoneActions`](types::PhoneActions) trait — the critical abstraction between
//!   the agent and phone hardware. All phone control goes through this trait,
//!   enabling the same agent code to work with real Android devices.

pub mod config;
pub mod error;
pub mod event;
pub mod message;
pub mod types;

pub use config::Config;
pub use error::{CoreError, Result};
pub use event::EventBus;
