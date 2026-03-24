use crate::{
    current_time_ms, fs_utils::write_json_private, FleetError, FleetKey, FleetToken,
    NodeCapability, NodeInfo, NodeRegistry, NodeStatus,
};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

const FLEET_KEY_FILE: &str = "fleet.key";
const NODES_FILE: &str = "nodes.json";
const TOKENS_FILE: &str = "tokens.json";

/// High-level fleet management tying together tokens, registry, and persistence.
pub struct FleetManager {
    fleet_dir: PathBuf,
    /// Signing key for task payload signatures (used in Phase 2 dispatch).
    key: FleetKey,
    registry: NodeRegistry,
    tokens: Vec<FleetToken>,
}

impl FleetManager {
    /// Initialize a new fleet directory with a signing key and empty state files.
    pub fn init(fleet_dir: &Path) -> Result<Self, FleetError> {
        fs::create_dir_all(fleet_dir)?;
        let manager = Self::empty(fleet_dir, FleetKey::generate()?);
        manager.key.save(&fleet_key_path(fleet_dir))?;
        manager.persist()?;
        Ok(manager)
    }

    /// Load an existing fleet directory from disk.
    pub fn load(fleet_dir: &Path) -> Result<Self, FleetError> {
        Ok(Self {
            fleet_dir: fleet_dir.to_path_buf(),
            key: FleetKey::load(&fleet_key_path(fleet_dir))?,
            registry: load_registry(&nodes_path(fleet_dir))?,
            tokens: load_tokens(&tokens_path(fleet_dir))?,
        })
    }

    /// Add a node, issue a token, and persist the updated fleet state.
    pub fn add_node(&mut self, name: &str, ip: &str, port: u16) -> Result<FleetToken, FleetError> {
        self.ensure_name_available(name)?;
        let node = build_node_info(&self.registry, name, ip, port);
        let token = FleetToken::generate(&node.node_id)?;
        self.register_node(node, token.clone())?;
        Ok(token)
    }

    /// Remove a node, revoke its token(s), and persist the updated fleet state.
    pub fn remove_node(&mut self, name: &str) -> Result<(), FleetError> {
        let removed = self.remove_registered_node(name)?;
        let revoked_indices = self.revoke_node_tokens(&removed.node_id);

        if let Err(error) = self.persist() {
            self.restore_removed_node(removed, &revoked_indices);
            return Err(error);
        }

        Ok(())
    }

    /// List all registered nodes.
    pub fn list_nodes(&self) -> Vec<&NodeInfo> {
        self.registry.list()
    }

    /// Verify a presented bearer token and return the matching node id.
    pub fn verify_bearer(&self, bearer: &str) -> Option<String> {
        self.tokens
            .iter()
            .find(|token| !token.revoked && token.verify_secret(bearer))
            .map(|token| token.node_id.clone())
    }

    /// Mark a worker as online, store its reported capabilities, and record a heartbeat.
    pub fn register_worker(
        &mut self,
        node_id: &str,
        capabilities: Vec<NodeCapability>,
        now_ms: u64,
    ) -> Result<NodeInfo, FleetError> {
        self.update_registered_node(node_id, move |registry| {
            if let Some(node) = registry.get_mut(node_id) {
                node.capabilities = capabilities;
                node.status = NodeStatus::Online;
            }
            let _ = registry.heartbeat(node_id, now_ms);
        })
    }

    /// Update worker liveness and current availability state from a heartbeat.
    pub fn record_worker_heartbeat(
        &mut self,
        node_id: &str,
        status: NodeStatus,
        now_ms: u64,
    ) -> Result<(), FleetError> {
        self.update_registered_node(node_id, move |registry| {
            if let Some(node) = registry.get_mut(node_id) {
                node.status = status;
            }
            let _ = registry.heartbeat(node_id, now_ms);
        })?;
        Ok(())
    }

    /// Mark that a worker completed a callback to the primary.
    pub fn mark_result_received(&mut self, node_id: &str, now_ms: u64) -> Result<(), FleetError> {
        self.record_worker_heartbeat(node_id, NodeStatus::Online, now_ms)
    }

    /// Persist the current registry and issued token state.
    pub fn persist(&self) -> Result<(), FleetError> {
        fs::create_dir_all(&self.fleet_dir)?;
        let nodes = sorted_nodes(&self.registry);
        let tokens = sorted_tokens(&self.tokens);
        persist_state(
            &nodes_path(&self.fleet_dir),
            &nodes,
            &tokens_path(&self.fleet_dir),
            &tokens,
        )
    }

    fn update_registered_node<F>(
        &mut self,
        node_id: &str,
        update: F,
    ) -> Result<NodeInfo, FleetError>
    where
        F: FnOnce(&mut NodeRegistry),
    {
        let original = self
            .registry
            .get(node_id)
            .cloned()
            .ok_or(FleetError::NodeNotFound)?;
        update(&mut self.registry);
        if let Err(error) = self.persist() {
            self.registry.register(original);
            return Err(error);
        }
        self.registry
            .get(node_id)
            .cloned()
            .ok_or(FleetError::NodeNotFound)
    }

    fn empty(fleet_dir: &Path, key: FleetKey) -> Self {
        Self {
            fleet_dir: fleet_dir.to_path_buf(),
            key,
            registry: NodeRegistry::new(),
            tokens: Vec::new(),
        }
    }

    fn ensure_name_available(&self, name: &str) -> Result<(), FleetError> {
        if self.registry.list().iter().any(|node| node.name == name) {
            Err(FleetError::DuplicateNode)
        } else {
            Ok(())
        }
    }

    fn register_node(&mut self, node: NodeInfo, token: FleetToken) -> Result<(), FleetError> {
        let node_id = node.node_id.clone();
        self.registry.register(node);
        self.tokens.push(token);

        if let Err(error) = self.persist() {
            self.tokens.pop();
            self.registry.remove(&node_id);
            return Err(error);
        }

        Ok(())
    }

    fn remove_registered_node(&mut self, name: &str) -> Result<NodeInfo, FleetError> {
        let node_id = self
            .find_node_id_by_name(name)
            .ok_or(FleetError::NodeNotFound)?;
        self.registry
            .remove(&node_id)
            .ok_or(FleetError::NodeNotFound)
    }

    fn find_node_id_by_name(&self, name: &str) -> Option<String> {
        self.registry
            .list()
            .into_iter()
            .find(|node| node.name == name)
            .map(|node| node.node_id.clone())
    }

    fn revoke_node_tokens(&mut self, node_id: &str) -> Vec<usize> {
        self.tokens
            .iter_mut()
            .enumerate()
            .filter_map(|(index, token)| revoke_token_if_active(index, token, node_id))
            .collect()
    }

    fn restore_removed_node(&mut self, node: NodeInfo, revoked_indices: &[usize]) {
        self.registry.register(node);
        for index in revoked_indices {
            if let Some(token) = self.tokens.get_mut(*index) {
                token.revoked = false;
            }
        }
    }
}

fn load_registry(path: &Path) -> Result<NodeRegistry, FleetError> {
    let nodes: Vec<NodeInfo> = load_json(path)?;
    Ok(registry_from_nodes(nodes))
}

fn load_tokens(path: &Path) -> Result<Vec<FleetToken>, FleetError> {
    load_json(path)
}

fn load_json<T>(path: &Path) -> Result<T, FleetError>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }

    let data = fs::read(path)?;
    Ok(serde_json::from_slice(&data)?)
}

fn persist_state(
    nodes_path: &Path,
    nodes: &[NodeInfo],
    tokens_path: &Path,
    tokens: &[FleetToken],
) -> Result<(), FleetError> {
    let nodes_tmp = temp_path(nodes_path);
    let tokens_tmp = temp_path(tokens_path);
    let result = persist_state_inner(
        nodes_path,
        nodes,
        &nodes_tmp,
        tokens_path,
        tokens,
        &tokens_tmp,
    );

    if result.is_err() {
        cleanup_temp_files(&[nodes_tmp, tokens_tmp]);
    }

    result
}

fn persist_state_inner(
    nodes_path: &Path,
    nodes: &[NodeInfo],
    nodes_tmp: &Path,
    tokens_path: &Path,
    tokens: &[FleetToken],
    tokens_tmp: &Path,
) -> Result<(), FleetError> {
    write_json_private(nodes_tmp, nodes)?;
    write_json_private(tokens_tmp, tokens)?;
    ensure_file_target(tokens_path)?;
    ensure_file_target(nodes_path)?;
    fs::rename(tokens_tmp, tokens_path)?;
    fs::rename(nodes_tmp, nodes_path)?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".tmp");
    PathBuf::from(temp)
}

fn ensure_file_target(path: &Path) -> Result<(), FleetError> {
    if path.is_dir() {
        let message = format!("refusing to replace directory target: {}", path.display());
        return Err(FleetError::IoError(std::io::Error::other(message)));
    }

    Ok(())
}

fn cleanup_temp_files(paths: &[PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

fn registry_from_nodes(nodes: Vec<NodeInfo>) -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    for node in nodes {
        registry.register(node);
    }
    registry
}

fn sorted_nodes(registry: &NodeRegistry) -> Vec<NodeInfo> {
    let mut nodes: Vec<NodeInfo> = registry.list().into_iter().cloned().collect();
    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    nodes
}

fn sorted_tokens(tokens: &[FleetToken]) -> Vec<FleetToken> {
    let mut tokens = tokens.to_vec();
    tokens.sort_by(|left, right| {
        left.node_id
            .cmp(&right.node_id)
            .then(left.token_id.cmp(&right.token_id))
    });
    tokens
}

fn revoke_token_if_active(index: usize, token: &mut FleetToken, node_id: &str) -> Option<usize> {
    if token.node_id != node_id || token.revoked {
        return None;
    }

    token.revoke();
    Some(index)
}

fn build_node_info(registry: &NodeRegistry, name: &str, ip: &str, port: u16) -> NodeInfo {
    NodeInfo {
        node_id: generate_node_id(registry, name),
        name: name.to_string(),
        endpoint: format!("https://{ip}:{port}"),
        auth_token: None,
        capabilities: Vec::new(),
        status: NodeStatus::Offline,
        last_heartbeat_ms: 0,
        registered_at_ms: current_time_ms(),
        address: Some(ip.to_string()),
        ssh_user: None,
        ssh_key: None,
    }
}

fn generate_node_id(registry: &NodeRegistry, name: &str) -> String {
    let slug = slugify_node_name(name);
    let timestamp = current_time_ms();
    let candidate = format!("{slug}-{timestamp:08x}");
    if registry.get(&candidate).is_none() {
        return candidate;
    }

    let mut suffix = 1_u32;
    loop {
        let candidate = format!("{slug}-{timestamp:08x}-{suffix}");
        if registry.get(&candidate).is_none() {
            return candidate;
        }
        suffix += 1;
    }
}

fn slugify_node_name(name: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;

    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            pending_dash = false;
            continue;
        }

        pending_dash = !slug.is_empty();
    }

    if slug.is_empty() {
        "node".to_string()
    } else {
        slug
    }
}

fn fleet_key_path(fleet_dir: &Path) -> PathBuf {
    fleet_dir.join(FLEET_KEY_FILE)
}

fn nodes_path(fleet_dir: &Path) -> PathBuf {
    fleet_dir.join(NODES_FILE)
}

fn tokens_path(fleet_dir: &Path) -> PathBuf {
    fleet_dir.join(TOKENS_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_utils::assert_private_permissions;
    use tempfile::TempDir;

    #[test]
    fn init_creates_directory_and_key() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");

        let manager = FleetManager::init(&fleet_dir).expect("fleet should initialize");

        assert!(fleet_dir.is_dir());
        assert!(fleet_key_path(&fleet_dir).is_file());
        assert!(nodes_path(&fleet_dir).is_file());
        assert!(tokens_path(&fleet_dir).is_file());
        assert!(manager.list_nodes().is_empty());
    }

    #[test]
    fn init_creates_fleet_key_file() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");

        FleetManager::init(&fleet_dir).expect("fleet should initialize");

        let key_bytes = fs::read(fleet_key_path(&fleet_dir)).expect("fleet key should exist");
        assert_eq!(key_bytes.len(), 32);
    }

    #[test]
    fn init_creates_private_state_files() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");

        FleetManager::init(&fleet_dir).expect("fleet should initialize");

        assert_private_permissions(&nodes_path(&fleet_dir));
        assert_private_permissions(&tokens_path(&fleet_dir));
    }

    #[test]
    fn add_node_generates_token_and_registers() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");

        let token = manager
            .add_node("Node Alpha", "10.0.0.2", 8400)
            .expect("node should add");
        let node = manager
            .list_nodes()
            .pop()
            .expect("node should be registered");

        assert_eq!(token.node_id, node.node_id);
        assert_ne!(token.node_id, node.name);
        assert!(token.node_id.starts_with("mac-mini-"));
        assert_eq!(node.name, "Node Alpha");
        assert_eq!(node.endpoint, "https://10.0.0.2:8400");
        assert_eq!(node.address.as_deref(), Some("10.0.0.2"));
        assert_eq!(node.status, NodeStatus::Offline);
    }

    #[test]
    fn add_duplicate_node_returns_error() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");

        manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("first node should add");
        let result = manager.add_node("node-alpha", "10.0.0.3", 8400);

        assert!(matches!(result, Err(FleetError::DuplicateNode)));
    }

    #[test]
    fn remove_node_revokes_token_and_deregisters() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        let token = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("node should add");

        manager
            .remove_node("node-alpha")
            .expect("node should remove cleanly");

        assert!(manager.list_nodes().is_empty());
        assert_eq!(manager.verify_bearer(&token.secret), None);
        assert_eq!(manager.tokens.len(), 1);
        assert!(manager.tokens[0].revoked);
    }

    #[test]
    fn remove_nonexistent_node_returns_error() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");

        let result = manager.remove_node("missing");

        assert!(matches!(result, Err(FleetError::NodeNotFound)));
    }

    #[test]
    fn verify_bearer_accepts_valid_token() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        let token = manager
            .add_node("Node Alpha", "10.0.0.2", 8400)
            .expect("node should add");

        let verified = manager.verify_bearer(&token.secret);

        assert_eq!(verified.as_deref(), Some(token.node_id.as_str()));
    }

    #[test]
    fn verify_bearer_rejects_revoked_token() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        let token = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("node should add");
        manager
            .remove_node("node-alpha")
            .expect("node should remove cleanly");

        let verified = manager.verify_bearer(&token.secret);

        assert_eq!(verified, None);
    }

    #[test]
    fn verify_bearer_rejects_unknown_token() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");

        assert_eq!(manager.verify_bearer("unknown-token"), None);
    }

    #[test]
    fn register_worker_updates_status_capabilities_and_heartbeat() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        let token = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("node should add");

        let node = manager
            .register_worker(
                &token.node_id,
                vec![
                    NodeCapability::AgenticLoop,
                    NodeCapability::Custom("macos-aarch64".into()),
                ],
                12_345,
            )
            .expect("worker should register");

        assert_eq!(node.status, NodeStatus::Online);
        assert_eq!(node.last_heartbeat_ms, 12_345);
        assert!(node.capabilities.contains(&NodeCapability::AgenticLoop));
    }

    #[test]
    fn record_worker_heartbeat_updates_node_status() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        let token = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("node should add");

        manager
            .record_worker_heartbeat(&token.node_id, NodeStatus::Busy, 54_321)
            .expect("heartbeat should persist");
        let node = manager
            .list_nodes()
            .into_iter()
            .find(|node| node.node_id == token.node_id)
            .expect("node should remain registered");

        assert_eq!(node.status, NodeStatus::Busy);
        assert_eq!(node.last_heartbeat_ms, 54_321);
    }

    #[test]
    fn mark_result_received_marks_worker_online() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        let token = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("node should add");
        manager
            .record_worker_heartbeat(&token.node_id, NodeStatus::Busy, 100)
            .expect("heartbeat should persist");

        manager
            .mark_result_received(&token.node_id, 200)
            .expect("result callback should persist");
        let node = manager
            .list_nodes()
            .into_iter()
            .find(|node| node.node_id == token.node_id)
            .expect("node should remain registered");

        assert_eq!(node.status, NodeStatus::Online);
        assert_eq!(node.last_heartbeat_ms, 200);
    }

    #[test]
    fn persist_and_load_roundtrip() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut manager = FleetManager::init(&fleet_dir).expect("fleet should initialize");
        let active = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("first node should add");
        let revoked = manager
            .add_node("node-beta", "10.0.0.3", 8401)
            .expect("second node should add");
        manager
            .remove_node("node-beta")
            .expect("node should remove cleanly");

        let loaded = FleetManager::load(&fleet_dir).expect("fleet should load");
        let node_names = sorted_node_names(loaded.list_nodes());

        assert_eq!(node_names, vec!["node-alpha".to_string()]);
        assert_eq!(
            loaded.verify_bearer(&active.secret).as_deref(),
            Some(active.node_id.as_str())
        );
        assert_eq!(loaded.verify_bearer(&revoked.secret), None);
        assert_eq!(loaded.tokens.len(), 2);
        assert_eq!(
            loaded.tokens.iter().filter(|token| token.revoked).count(),
            1
        );
    }

    #[test]
    fn persist_writes_private_state_files_and_removes_temp_files() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");

        manager
            .add_node("Node Alpha", "10.0.0.2", 8400)
            .expect("node should add");

        assert_private_permissions(&nodes_path(temp_dir.path()));
        assert_private_permissions(&tokens_path(temp_dir.path()));
        assert!(!temp_path(&nodes_path(temp_dir.path())).exists());
        assert!(!temp_path(&tokens_path(temp_dir.path())).exists());
    }

    #[test]
    fn persist_cleans_up_temp_files_when_target_is_directory() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let manager = FleetManager::empty(
            &fleet_dir,
            FleetKey::generate().expect("fleet key should generate"),
        );

        fs::create_dir_all(tokens_path(&fleet_dir)).expect("tokens target directory should exist");

        let result = manager.persist();

        assert!(matches!(result, Err(FleetError::IoError(_))));
        assert!(!nodes_path(&fleet_dir).exists());
        assert!(tokens_path(&fleet_dir).is_dir());
        assert!(!temp_path(&nodes_path(&fleet_dir)).exists());
        assert!(!temp_path(&tokens_path(&fleet_dir)).exists());
    }

    #[test]
    fn list_nodes_returns_registered_nodes() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("first node should add");
        manager
            .add_node("node-beta", "10.0.0.3", 8401)
            .expect("second node should add");

        let names = sorted_node_names(manager.list_nodes());

        assert_eq!(
            names,
            vec!["node-beta".to_string(), "node-alpha".to_string()]
        );
    }

    fn sorted_node_names(nodes: Vec<&NodeInfo>) -> Vec<String> {
        let mut names: Vec<String> = nodes.into_iter().map(|node| node.name.clone()).collect();
        names.sort();
        names
    }
}
