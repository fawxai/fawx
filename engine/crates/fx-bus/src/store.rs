use crate::{BusError, Envelope};
use fx_session::SessionKey;
use fx_storage::Storage;

const BUS_QUEUE_TABLE: &str = "bus_queue";

/// Persistent queue for offline message delivery.
/// Uses redb table: key = "{to_session}:{message_id}" → serialized Envelope JSON.
#[derive(Clone)]
pub struct BusStore {
    storage: Storage,
}

impl BusStore {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn enqueue(&self, envelope: &Envelope) -> Result<(), BusError> {
        let bytes = serde_json::to_vec(envelope)?;
        self.storage
            .put(BUS_QUEUE_TABLE, &queue_key(envelope), &bytes)?;
        Ok(())
    }

    pub fn read_pending(&self, session_key: &SessionKey) -> Result<Vec<Envelope>, BusError> {
        let keys = self.queue_keys(session_key)?;
        let envelopes = self.load_envelopes(&keys)?;
        Ok(sort_envelopes(envelopes))
    }

    pub fn delete(&self, session_key: &SessionKey, envelope_id: &str) -> Result<bool, BusError> {
        let key = queue_key_parts(session_key, envelope_id);
        Ok(self.storage.delete(BUS_QUEUE_TABLE, &key)?)
    }

    pub fn delete_batch(
        &self,
        session_key: &SessionKey,
        envelope_ids: &[String],
    ) -> Result<(), BusError> {
        let keys = queue_keys_for_ids(session_key, envelope_ids);
        self.storage.delete_many(BUS_QUEUE_TABLE, &keys)?;
        Ok(())
    }

    pub fn count(&self, session_key: &SessionKey) -> Result<usize, BusError> {
        Ok(self.queue_keys(session_key)?.len())
    }

    fn queue_keys(&self, session_key: &SessionKey) -> Result<Vec<String>, BusError> {
        let prefix = queue_prefix(session_key);
        // Full table scan acceptable for V1 volume. For high-throughput scenarios,
        // consider a secondary index keyed by session key.
        let mut keys = self
            .storage
            .list_keys(BUS_QUEUE_TABLE)?
            .into_iter()
            .filter(|key| key.starts_with(&prefix))
            .collect::<Vec<_>>();
        keys.sort();
        Ok(keys)
    }

    fn load_envelopes(&self, keys: &[String]) -> Result<Vec<Envelope>, BusError> {
        let mut envelopes = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(bytes) = self.storage.get(BUS_QUEUE_TABLE, key)? {
                envelopes.push(serde_json::from_slice(&bytes)?);
            }
        }
        Ok(envelopes)
    }
}

fn queue_key(envelope: &Envelope) -> String {
    queue_key_parts(&envelope.to, &envelope.id)
}

fn queue_key_parts(session_key: &SessionKey, envelope_id: &str) -> String {
    format!("{}:{envelope_id}", session_key.as_str())
}

fn queue_prefix(session_key: &SessionKey) -> String {
    format!("{}:", session_key.as_str())
}

fn queue_keys_for_ids(session_key: &SessionKey, envelope_ids: &[String]) -> Vec<String> {
    envelope_ids
        .iter()
        .map(|envelope_id| queue_key_parts(session_key, envelope_id))
        .collect()
}

fn sort_envelopes(mut envelopes: Vec<Envelope>) -> Vec<Envelope> {
    envelopes.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    envelopes
}
