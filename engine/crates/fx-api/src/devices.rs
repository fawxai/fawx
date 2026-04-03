use anyhow::{Context, Result};
use rand::Rng;
use ring::digest;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

const DEVICE_TOKEN_PREFIX: &str = "fawx_pat_";
const DEVICE_TOKEN_LENGTH: usize = 32;
const TOKEN_CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceToken {
    pub id: String,
    pub token_hash: String,
    pub device_name: String,
    pub created_at: u64,
    pub last_used_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub device_name: String,
    pub created_at: u64,
    pub last_used_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceStore {
    #[serde(default)]
    devices: Vec<DeviceToken>,
}

impl From<&DeviceToken> for DeviceInfo {
    fn from(device: &DeviceToken) -> Self {
        Self {
            id: device.id.clone(),
            device_name: device.device_name.clone(),
            created_at: device.created_at,
            last_used_at: device.last_used_at,
        }
    }
}

impl DeviceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list_devices(&self) -> &[DeviceToken] {
        &self.devices
    }

    pub fn list_device_info(&self) -> Vec<DeviceInfo> {
        self.devices.iter().map(DeviceInfo::from).collect()
    }

    #[cfg(test)]
    fn list_devices_mut(&mut self) -> &mut Vec<DeviceToken> {
        &mut self.devices
    }

    pub fn create_device(&mut self, name: &str) -> (String, DeviceToken) {
        let raw_token = format!("{DEVICE_TOKEN_PREFIX}{}", random_token_body());
        let timestamp = current_time_seconds();
        let device = DeviceToken {
            id: format!("dev-{}", Uuid::new_v4().simple()),
            token_hash: hash_token(&raw_token),
            device_name: name.trim().to_string(),
            created_at: timestamp,
            last_used_at: timestamp,
        };
        self.devices.push(device.clone());
        (raw_token, device)
    }

    pub fn authenticate(&mut self, bearer_token: &str) -> Option<String> {
        let token_hash = hash_token(bearer_token);
        let device = self
            .devices
            .iter_mut()
            .find(|device| device.token_hash == token_hash)?;
        device.last_used_at = current_time_seconds();
        Some(device.id.clone())
    }

    pub fn revoke(&mut self, device_id: &str) -> Option<DeviceToken> {
        let index = self
            .devices
            .iter()
            .position(|device| device.id == device_id)?;
        Some(self.devices.remove(index))
    }

    pub(crate) fn restore_device(&mut self, device: DeviceToken) {
        self.devices.push(device);
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        ensure_parent_dir(path)?;
        let bytes = serialize_store(self)?;
        fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
        set_private_permissions(path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Self {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => return handle_load_error(path, error),
        };
        parse_store(path, &bytes)
    }

    fn normalize_timestamps(&mut self) {
        for device in &mut self.devices {
            normalize_device_timestamps(device);
        }
    }
}

/// Best-effort persist of device store to disk.
pub fn persist_devices(devices: &DeviceStore, path: Option<&Path>) {
    let Some(path) = path else {
        return;
    };
    if let Err(error) = devices.save(path) {
        tracing::warn!(path = %path.display(), error = %error, "device store save failed");
    }
}

fn random_token_body() -> String {
    let mut rng = rand::thread_rng();
    (0..DEVICE_TOKEN_LENGTH)
        .map(|_| {
            let index = rng.gen_range(0..TOKEN_CHARSET.len());
            char::from(TOKEN_CHARSET[index])
        })
        .collect()
}

fn hash_token(token: &str) -> String {
    let hash = digest::digest(&digest::SHA256, token.as_bytes());
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn current_time_seconds() -> u64 {
    crate::time_util::current_time_seconds()
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    Ok(())
}

fn serialize_store(store: &DeviceStore) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(store).context("failed to serialize device store")?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn handle_load_error(path: &Path, error: std::io::Error) -> DeviceStore {
    if error.kind() == std::io::ErrorKind::NotFound {
        return DeviceStore::new();
    }
    tracing::warn!(path = %path.display(), error = %error, "failed to read device store");
    DeviceStore::new()
}

fn parse_store(path: &Path, bytes: &[u8]) -> DeviceStore {
    match serde_json::from_slice::<DeviceStore>(bytes) {
        Ok(mut store) => {
            store.normalize_timestamps();
            store
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "failed to parse device store");
            DeviceStore::new()
        }
    }
}

fn normalize_device_timestamps(device: &mut DeviceToken) {
    device.created_at = normalize_timestamp(device.created_at);
    device.last_used_at = normalize_timestamp(device.last_used_at);
}

fn normalize_timestamp(timestamp: u64) -> u64 {
    crate::time_util::normalize_timestamp(timestamp)
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to chmod {} to 0600", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_device_returns_hashed_token() {
        let mut store = DeviceStore::new();
        let (raw_token, device) = store.create_device("Alice's MacBook");

        assert!(raw_token.starts_with(DEVICE_TOKEN_PREFIX));
        assert_eq!(
            raw_token.len(),
            DEVICE_TOKEN_PREFIX.len() + DEVICE_TOKEN_LENGTH
        );
        assert_eq!(store.list_devices()[0], device);
        assert_ne!(device.token_hash, raw_token);
        assert_eq!(device.token_hash.len(), 64);
    }

    #[test]
    fn list_device_info_excludes_token_hash() {
        let mut store = DeviceStore::new();
        let _ = store.create_device("Alice's MacBook");

        let json = serde_json::to_value(store.list_device_info()).expect("serialize device info");

        assert!(json[0].get("token_hash").is_none());
        assert_eq!(json[0]["device_name"], "Alice's MacBook");
    }

    #[test]
    fn authenticate_works() {
        let mut store = DeviceStore::new();
        let (raw_token, device) = store.create_device("Alice's MacBook");
        store.list_devices_mut()[0].last_used_at = 0;

        assert_eq!(store.authenticate(&raw_token), Some(device.id));
        assert!(store.list_devices()[0].last_used_at > 0);
        assert!(store.authenticate("fawx_pat_wrong").is_none());
    }

    #[test]
    fn revoke_invalidates_device() {
        let mut store = DeviceStore::new();
        let (raw_token, device) = store.create_device("Alice's MacBook");

        assert_eq!(store.revoke(&device.id), Some(device.clone()));
        assert!(store.revoke(&device.id).is_none());
        assert!(store.authenticate(&raw_token).is_none());
    }

    #[test]
    fn save_load_roundtrip() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("devices.json");
        let mut store = DeviceStore::new();
        let (raw_token, _) = store.create_device("Alice's MacBook");

        store.save(&path).expect("save device store");
        let mut loaded = DeviceStore::load(&path);

        assert_eq!(loaded.list_devices().len(), 1);
        assert!(loaded.authenticate(&raw_token).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("devices.json");
        let mut store = DeviceStore::new();
        let _ = store.create_device("Alice's MacBook");

        store.save(&path).expect("save device store");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
    }

    #[test]
    fn load_normalizes_legacy_millisecond_timestamps() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("devices.json");
        let store = DeviceStore {
            devices: vec![DeviceToken {
                id: "dev-123".to_string(),
                token_hash: "hash".to_string(),
                device_name: "Alice's MacBook".to_string(),
                created_at: 1_700_000_000_000,
                last_used_at: 1_700_000_005_000,
            }],
        };
        fs::write(
            &path,
            serde_json::to_vec(&store).expect("serialize legacy store"),
        )
        .expect("write legacy store");

        let loaded = DeviceStore::load(&path);

        assert_eq!(loaded.list_devices()[0].created_at, 1_700_000_000);
        assert_eq!(loaded.list_devices()[0].last_used_at, 1_700_000_005);
    }

    #[test]
    fn load_returns_empty_store_on_corrupt_json() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("devices.json");
        fs::write(&path, b"not json").expect("write corrupt store");

        let loaded = DeviceStore::load(&path);

        assert!(loaded.list_devices().is_empty());
    }
}
