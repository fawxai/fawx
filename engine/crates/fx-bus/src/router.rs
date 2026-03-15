use crate::{BusError, BusStore, Envelope};
use fx_session::SessionKey;
use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::mpsc;

const CHANNEL_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendResult {
    pub envelope_id: String,
    pub delivered: bool,
}

#[derive(Clone)]
pub struct SessionBus {
    subscribers: Arc<RwLock<HashMap<SessionKey, mpsc::Sender<Envelope>>>>,
    store: BusStore,
}

impl SessionBus {
    pub fn new(store: BusStore) -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            store,
        }
    }

    pub fn subscribe(&self, session_key: &SessionKey) -> mpsc::Receiver<Envelope> {
        let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
        self.register(session_key, sender.clone());
        if let Err(error) = self.drain_offline(session_key, &sender) {
            tracing::warn!(session_key = %session_key, error = %error, "failed to drain offline bus messages");
        }
        receiver
    }

    pub fn unsubscribe(&self, session_key: &SessionKey) {
        self.subscribers_write().remove(session_key);
    }

    pub async fn send(&self, envelope: Envelope) -> Result<SendResult, BusError> {
        let envelope_id = envelope.id.clone();
        let delivered = self.try_deliver(envelope)?;
        Ok(SendResult {
            envelope_id,
            delivered,
        })
    }

    fn drain_offline(
        &self,
        session_key: &SessionKey,
        sender: &mpsc::Sender<Envelope>,
    ) -> Result<(), BusError> {
        let envelopes = self.store.read_pending(session_key)?;
        let delivered_ids = self.collect_delivered_ids(sender, envelopes)?;
        self.store.delete_batch(session_key, &delivered_ids)?;
        Ok(())
    }

    fn collect_delivered_ids(
        &self,
        sender: &mpsc::Sender<Envelope>,
        envelopes: Vec<Envelope>,
    ) -> Result<Vec<String>, BusError> {
        let mut delivered_ids = Vec::new();
        for envelope in envelopes {
            match self.try_drain_delivery(sender, envelope)? {
                Some(envelope_id) => delivered_ids.push(envelope_id),
                None => break,
            }
        }
        Ok(delivered_ids)
    }

    fn try_drain_delivery(
        &self,
        sender: &mpsc::Sender<Envelope>,
        envelope: Envelope,
    ) -> Result<Option<String>, BusError> {
        let envelope_id = envelope.id.clone();
        match sender.try_send(envelope) {
            Ok(()) => Ok(Some(envelope_id)),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Ok(None),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(envelope)) => {
                self.unsubscribe(&envelope.to);
                Ok(None)
            }
        }
    }

    fn try_deliver(&self, envelope: Envelope) -> Result<bool, BusError> {
        let Some(sender) = self.subscriber(&envelope.to) else {
            self.store.enqueue(&envelope)?;
            return Ok(false);
        };
        self.deliver_or_store(&sender, envelope)
    }

    fn deliver_or_store(
        &self,
        sender: &mpsc::Sender<Envelope>,
        envelope: Envelope,
    ) -> Result<bool, BusError> {
        match sender.try_send(envelope) {
            Ok(()) => Ok(true),
            Err(tokio::sync::mpsc::error::TrySendError::Full(envelope)) => {
                self.store.enqueue(&envelope)?;
                Ok(false)
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(envelope)) => {
                self.unsubscribe(&envelope.to);
                self.store.enqueue(&envelope)?;
                Ok(false)
            }
        }
    }

    fn register(&self, session_key: &SessionKey, sender: mpsc::Sender<Envelope>) {
        self.subscribers_write().insert(session_key.clone(), sender);
    }

    fn subscriber(&self, session_key: &SessionKey) -> Option<mpsc::Sender<Envelope>> {
        self.subscribers_read().get(session_key).cloned()
    }

    fn subscribers_read(&self) -> RwLockReadGuard<'_, HashMap<SessionKey, mpsc::Sender<Envelope>>> {
        match self.subscribers.read() {
            Ok(guard) => guard,
            Err(error) => error.into_inner(),
        }
    }

    fn subscribers_write(
        &self,
    ) -> RwLockWriteGuard<'_, HashMap<SessionKey, mpsc::Sender<Envelope>>> {
        match self.subscribers.write() {
            Ok(guard) => guard,
            Err(error) => error.into_inner(),
        }
    }
}
