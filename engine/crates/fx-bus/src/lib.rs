mod envelope;
mod router;
mod store;

pub use envelope::{Envelope, Payload};
pub use router::{SendResult, SessionBus};
pub use store::BusStore;

#[derive(Debug, thiserror::Error)]
pub enum BusError {
    #[error("storage error: {0}")]
    Storage(#[from] fx_core::error::StorageError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("channel closed")]
    ChannelClosed,
}

#[cfg(test)]
mod tests;
