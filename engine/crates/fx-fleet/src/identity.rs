use crate::{fs_utils::write_json_private, FleetError};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;

const REDACTED: &str = "[REDACTED]";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetIdentity {
    pub node_id: String,
    pub primary_endpoint: String,
    pub bearer_token: String,
    pub registered_at_ms: u64,
}

impl fmt::Debug for FleetIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FleetIdentity")
            .field("node_id", &self.node_id)
            .field("primary_endpoint", &self.primary_endpoint)
            .field("bearer_token", &REDACTED)
            .field("registered_at_ms", &self.registered_at_ms)
            .finish()
    }
}

impl FleetIdentity {
    pub fn save(&self, path: &Path) -> Result<(), FleetError> {
        write_json_private(path, self)
    }

    pub fn load(path: &Path) -> Result<Self, FleetError> {
        let bytes = fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_utils::assert_private_permissions;
    use tempfile::TempDir;

    fn sample_identity() -> FleetIdentity {
        FleetIdentity {
            node_id: "build-node-a1b2c3".to_string(),
            primary_endpoint: "http://192.0.2.1:8400".to_string(),
            bearer_token: "tok_secret_123".to_string(),
            registered_at_ms: 12345,
        }
    }

    #[test]
    fn fleet_identity_save_load_roundtrip() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let path = temp_dir.path().join("fleet/identity.json");
        let identity = sample_identity();

        identity.save(&path).expect("identity should save");
        let loaded = FleetIdentity::load(&path).expect("identity should load");

        assert_eq!(loaded, identity);
        assert_private_permissions(&path);
    }

    #[test]
    fn fleet_identity_debug_redacts_token() {
        let identity = sample_identity();
        let debug_output = format!("{identity:?}");

        assert!(debug_output.contains(REDACTED));
        assert!(!debug_output.contains(&identity.bearer_token));
    }
}
