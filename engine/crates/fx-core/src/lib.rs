//! Core types, configuration, event bus, and error handling for Fawx.
//!
//! This crate provides the foundational types and utilities used across
//! all Fawx crates. It includes:
//! - Configuration management
//! - Inter-crate messaging types
//! - Event bus for asynchronous communication
//! - Error taxonomy
//! - Shared types (intents, action plans, etc.)
//! - [`PhoneActions`](types::PhoneActions) trait — the critical abstraction between
//!   the agent and phone hardware. All phone control goes through this trait,
//!   enabling the same agent code to work with real Android devices.

pub mod channel;
pub mod config;
pub mod error;
pub mod event;
pub mod events;
pub mod kernel_manifest;
pub mod memory;
pub mod message;
pub mod runtime_info;
pub mod self_modify;
pub mod signals;
pub mod types;

pub use config::Config;
pub use error::{CoreError, Result};
pub use event::EventBus;
