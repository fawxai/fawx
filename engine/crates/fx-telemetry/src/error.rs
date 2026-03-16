#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("consent error: {0}")]
    ConsentError(String),
    #[error("buffer full ({0} signals)")]
    BufferFull(usize),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
