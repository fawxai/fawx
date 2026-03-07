//! Error types for Fawx.
//!
//! Defines a comprehensive error taxonomy used across all crates.

use thiserror::Error;

/// Result type alias using `CoreError`
pub type Result<T> = std::result::Result<T, CoreError>;

/// Core errors that can occur in any Fawx crate.
#[derive(Error, Debug)]
pub enum CoreError {
    /// Configuration file could not be loaded
    #[error("Failed to load configuration: {0}")]
    ConfigLoad(String),

    /// Configuration file could not be parsed
    #[error("Failed to parse configuration: {0}")]
    ConfigParse(String),

    /// Event bus error
    #[error("Event bus error: {0}")]
    EventBus(String),

    /// Generic I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// LLM-related errors (local and cloud)
#[derive(Error, Debug)]
pub enum LlmError {
    /// Model file not found or invalid
    #[error("Model error: {0}")]
    Model(String),

    /// Inference failed
    #[error("Inference failed: {0}")]
    Inference(String),

    /// API request failed (cloud LLM)
    #[error("API request failed: {0}")]
    ApiRequest(String),

    /// Invalid response from LLM
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

/// Storage-related errors
#[derive(Error, Debug)]
pub enum StorageError {
    /// Database operation failed
    #[error("Database error: {0}")]
    Database(String),

    /// Encryption/decryption failed
    #[error("Encryption error: {0}")]
    Encryption(String),

    /// Key not found
    #[error("Key not found: {0}")]
    KeyNotFound(String),
}

/// Security-related errors
#[derive(Error, Debug)]
pub enum SecurityError {
    /// Policy violation
    #[error("Policy violation: {0}")]
    PolicyViolation(String),

    /// Capability denied
    #[error("Capability denied: {0}")]
    CapabilityDenied(String),

    /// Signature verification failed
    #[error("Signature verification failed: {0}")]
    SignatureVerification(String),

    /// Audit log error
    #[error("Audit log error: {0}")]
    AuditLog(String),
}

/// Skill (WASM) related errors
#[derive(Error, Debug)]
pub enum SkillError {
    /// Skill loading failed
    #[error("Failed to load skill: {0}")]
    Load(String),

    /// Skill execution failed
    #[error("Skill execution failed: {0}")]
    Execution(String),

    /// Invalid skill manifest
    #[error("Invalid skill manifest: {0}")]
    InvalidManifest(String),

    /// Feature not supported by this API version
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

/// Phone action errors (Android-specific for PoC, abstracted for OS)
#[derive(Error, Debug)]
pub enum PhoneError {
    /// Touch injection failed
    #[error("Touch injection failed: {0}")]
    TouchInjection(String),

    /// Screen capture failed
    #[error("Screen capture failed: {0}")]
    ScreenCapture(String),

    /// App not found
    #[error("App not found: {0}")]
    AppNotFound(String),

    /// Action execution failed
    #[error("Action execution failed: {0}")]
    ActionFailed(String),
}
