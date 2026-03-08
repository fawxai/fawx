//! Error types for journal operations.

use std::fmt;

/// Errors that can occur during journal operations.
#[derive(Debug)]
pub enum JournalError {
    /// I/O error (file read/write/create).
    Io(std::io::Error),
    /// Serialization or deserialization error.
    Serialization(String),
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "journal I/O error: {err}"),
            Self::Serialization(msg) => write!(f, "journal serialization error: {msg}"),
        }
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Serialization(_) => None,
        }
    }
}

impl From<std::io::Error> for JournalError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for JournalError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}
