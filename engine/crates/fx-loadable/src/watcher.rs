//! Filesystem watcher for hot-reloading WASM skills.
//!
//! [`SkillWatcher`] monitors `~/.fawx/skills/` for new, updated, or removed
//! WASM skill binaries and hot-swaps them into the running [`SkillRegistry`]
//! without restart. Changes are debounced per skill directory (500ms) and
//! deduplicated by SHA-256 hash to avoid spurious reloads.

use crate::lifecycle::SkillLifecycleManager;
use crate::skill::SkillError;
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Events emitted by the [`SkillWatcher`] when skills change.
#[derive(Debug, Clone)]
pub enum ReloadEvent {
    /// A new skill was loaded for the first time.
    Loaded {
        skill_name: String,
        version: String,
        revision: String,
        source: String,
    },
    /// An existing skill was updated with a new binary.
    Updated {
        skill_name: String,
        old_version: String,
        new_version: String,
        revision: String,
        source: String,
    },
    /// A skill was removed (directory deleted or manifest/wasm missing).
    Removed { skill_name: String },
    /// An error occurred while loading a skill.
    Error { skill_name: String, error: String },
}

/// Tracks the last known state of a loaded skill.
struct SkillState {
    hash: String,
    version: String,
}

/// Watches `~/.fawx/skills/` and hot-reloads WASM skills into the registry.
///
/// The watcher debounces filesystem events per skill directory (500ms window)
/// and compares SHA-256 hashes to avoid spurious reloads when the binary
/// hasn't actually changed.
pub struct SkillWatcher {
    skills_dir: PathBuf,
    lifecycle: Arc<std::sync::Mutex<SkillLifecycleManager>>,
    event_tx: mpsc::Sender<ReloadEvent>,
    hashes: HashMap<String, SkillState>,
}

/// Debounce window for filesystem events (per skill directory).
const DEBOUNCE_MS: u64 = 500;

impl SkillWatcher {
    /// Create a new watcher with an empty hashes map.
    ///
    /// Call [`initialize_hashes`](Self::initialize_hashes) before [`run`](Self::run)
    /// to populate hashes for startup-loaded skills.
    pub fn new(
        skills_dir: PathBuf,
        lifecycle: Arc<std::sync::Mutex<SkillLifecycleManager>>,
        event_tx: mpsc::Sender<ReloadEvent>,
    ) -> Self {
        Self {
            skills_dir,
            lifecycle,
            event_tx,
            hashes: HashMap::new(),
        }
    }

    /// Scan the skills directory and populate hashes for existing `.wasm` files.
    ///
    /// Must be called before [`run`](Self::run) so the watcher can distinguish
    /// between new skills and updates to existing ones.
    pub fn initialize_hashes(&mut self) {
        let statuses = match self.lifecycle.lock() {
            Ok(lifecycle) => lifecycle.statuses(),
            Err(error) => {
                tracing::warn!(error = %error, "skill lifecycle lock poisoned");
                return;
            }
        };
        for status in statuses {
            self.hashes.insert(
                status.name,
                SkillState {
                    hash: status.activation.revision.revision_hash(),
                    version: status.activation.revision.version,
                },
            );
        }

        tracing::info!(
            count = self.hashes.len(),
            "initialized watcher hashes for existing skills"
        );
    }

    /// Run the watcher loop. This is async and runs forever until an
    /// unrecoverable error occurs or the process exits.
    ///
    /// If the `notify` watcher fails to initialize, logs the error and
    /// returns — Fawx continues with startup-loaded skills (no hot-reload).
    pub async fn run(mut self) -> Result<(), SkillError> {
        let (fs_tx, fs_rx) = std::sync::mpsc::channel();

        let _watcher = match create_watcher(&self.skills_dir, fs_tx) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "failed to start skill watcher — hot-reload disabled");
                return Err(e);
            }
        };

        tracing::info!(
            dir = %self.skills_dir.display(),
            "skill watcher started"
        );

        // Bridge sync notify channel → async tokio channel via a dedicated
        // blocking task. This avoids unsafe pointer casts and ensures events
        // are never lost when the select! loop picks the deadline branch.
        let (bridge_tx, bridge_rx) = tokio::sync::mpsc::channel(256);
        tokio::task::spawn_blocking(move || {
            bridge_sync_to_async(fs_rx, bridge_tx);
        });

        self.event_loop(bridge_rx).await
    }

    /// Core event loop: receives filesystem events, debounces them, and
    /// processes quiescent skill directories.
    async fn event_loop(
        &mut self,
        mut bridge_rx: tokio::sync::mpsc::Receiver<Vec<String>>,
    ) -> Result<(), SkillError> {
        let mut pending: HashMap<String, Instant> = HashMap::new();

        loop {
            let next_deadline = earliest_deadline(&pending);

            tokio::select! {
                result = bridge_rx.recv() => {
                    match result {
                        Some(names) => {
                            let deadline = Instant::now()
                                + std::time::Duration::from_millis(DEBOUNCE_MS);
                            for skill_name in names {
                                pending.insert(skill_name, deadline);
                            }
                        }
                        None => {
                            tracing::warn!("skill watcher bridge channel closed — stopping");
                            break Ok(());
                        }
                    }
                }
                _ = sleep_until_deadline(next_deadline) => {
                    let expired = collect_expired(&mut pending);
                    for skill_name in expired {
                        self.process_skill_change(&skill_name).await;
                    }
                }
            }
        }
    }

    /// Process a single skill directory change after debounce.
    async fn process_skill_change(&mut self, skill_name: &str) {
        let skill_dir = self.skills_dir.join(skill_name);

        if skill_dir_is_valid(&skill_dir, skill_name) {
            self.handle_load_or_update(skill_name, &skill_dir);
        } else {
            self.handle_removal(skill_name);
        }
    }

    /// Attempt to load or update a skill from its directory.
    fn handle_load_or_update(&mut self, skill_name: &str, skill_dir: &Path) {
        let mut lifecycle = self.lifecycle.lock().unwrap_or_else(|p| p.into_inner());
        let previous = lifecycle.active(skill_name).cloned();
        match lifecycle
            .stage_from_source(skill_dir)
            .and_then(|_| lifecycle.activate(skill_name))
        {
            Ok(changed) => {
                let current = lifecycle.active(skill_name).cloned();
                drop(lifecycle);
                self.apply_loaded_skill(skill_name, previous, current, changed)
            }
            Err(e) => {
                tracing::warn!(skill = %skill_name, error = %e, "failed to reload skill");
                let _ = self.event_tx.try_send(ReloadEvent::Error {
                    skill_name: skill_name.to_string(),
                    error: e,
                });
            }
        }
    }

    /// Apply a successfully loaded skill to the registry (new or update).
    fn apply_loaded_skill(
        &mut self,
        skill_name: &str,
        previous: Option<crate::lifecycle::SkillActivation>,
        current: Option<crate::lifecycle::SkillActivation>,
        changed: bool,
    ) {
        let mirrored = self.hashes.get(skill_name);
        let Some(current) = current else {
            tracing::warn!(skill = %skill_name, "reload reported success without active revision");
            return;
        };
        if !changed {
            tracing::debug!(
                skill = %skill_name,
                previous_hash = ?mirrored.as_ref().map(|state| state.hash.as_str()),
                "hash unchanged — skipping reload"
            );
            return;
        }

        let revision = current.revision.revision_hash();
        let source = current.source.display();
        let new_version = current.revision.version.clone();
        let event = if let Some(old) = previous {
            tracing::info!(
                skill = %skill_name,
                previous_version = ?mirrored.as_ref().map(|state| state.version.as_str()),
                version = %new_version,
                revision = %crate::lifecycle::short_hash(&revision),
                "updated WASM skill"
            );
            ReloadEvent::Updated {
                skill_name: skill_name.to_string(),
                old_version: old.revision.version,
                new_version: new_version.clone(),
                revision: revision.clone(),
                source: source.clone(),
            }
        } else {
            tracing::info!(skill = %skill_name, version = %new_version, revision = %crate::lifecycle::short_hash(&revision), "loaded new WASM skill");
            ReloadEvent::Loaded {
                skill_name: skill_name.to_string(),
                version: new_version.clone(),
                revision: revision.clone(),
                source: source.clone(),
            }
        };

        self.hashes.insert(
            skill_name.to_string(),
            SkillState {
                hash: revision,
                version: new_version,
            },
        );
        let _ = self.event_tx.try_send(event);
    }

    /// Handle removal of a skill directory.
    fn handle_removal(&mut self, skill_name: &str) {
        if self.hashes.remove(skill_name).is_some() {
            let _ = self
                .lifecycle
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove_skill(skill_name);
            tracing::info!(skill = %skill_name, "removed WASM skill");
            let _ = self.event_tx.try_send(ReloadEvent::Removed {
                skill_name: skill_name.to_string(),
            });
        }
    }
}

/// Bridge loop: reads from the sync `notify` channel and forwards batched
/// skill names to the async tokio channel. Runs on a dedicated blocking
/// thread for the entire watcher lifetime.
fn bridge_sync_to_async(
    rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    tx: tokio::sync::mpsc::Sender<Vec<String>>,
) {
    while let Ok(event) = rx.recv() {
        let mut names = HashSet::new();
        if let Ok(ref ev) = event {
            collect_skill_names_from_event(ev, &mut names);
        }
        // Drain any additional queued events to batch them
        while let Ok(event) = rx.try_recv() {
            if let Ok(ref ev) = event {
                collect_skill_names_from_event(ev, &mut names);
            }
        }
        if !names.is_empty() {
            let batch: Vec<String> = names.into_iter().collect();
            if tx.blocking_send(batch).is_err() {
                break; // receiver dropped, watcher shutting down
            }
        }
    }
}

/// Create a `notify` watcher on the skills directory.
fn create_watcher(
    skills_dir: &Path,
    tx: std::sync::mpsc::Sender<notify::Result<notify::Event>>,
) -> Result<notify::RecommendedWatcher, SkillError> {
    let mut watcher = notify::recommended_watcher(tx)
        .map_err(|e| format!("failed to create filesystem watcher: {e}"))?;

    watcher
        .watch(skills_dir, RecursiveMode::Recursive)
        .map_err(|e| format!("failed to watch {}: {e}", skills_dir.display()))?;

    Ok(watcher)
}

/// Check if a skill directory is valid (has manifest.toml + {name}.wasm).
fn skill_dir_is_valid(skill_dir: &Path, skill_name: &str) -> bool {
    skill_dir.is_dir()
        && skill_dir.join("manifest.toml").exists()
        && skill_dir.join(format!("{skill_name}.wasm")).exists()
}

/// Extract skill directory names from a notify event's paths.
fn collect_skill_names_from_event(event: &notify::Event, names: &mut HashSet<String>) {
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return;
    }

    for path in &event.paths {
        if is_lifecycle_path(path) {
            continue;
        }
        if let Some(name) = extract_skill_dir_name(path) {
            names.insert(name);
        }
    }
}

fn is_lifecycle_path(path: &Path) -> bool {
    path.components().any(|component| match component {
        std::path::Component::Normal(name) => {
            name.to_str() == Some(crate::lifecycle::SOURCE_METADATA_FILE)
                || name.to_str() == Some(".fawx-lifecycle")
        }
        _ => false,
    })
}

/// Extract the skill directory name from a file path.
///
/// Given `~/.fawx/skills/github/github.wasm`, returns `"github"`.
/// Given `~/.fawx/skills/github/`, returns `"github"`.
fn extract_skill_dir_name(path: &Path) -> Option<String> {
    let components: Vec<_> = path.components().collect();
    for (i, component) in components.iter().enumerate() {
        if let std::path::Component::Normal(name) = component {
            if name.to_str() == Some("skills") && i + 1 < components.len() {
                if let std::path::Component::Normal(skill_dir) = &components[i + 1] {
                    return skill_dir.to_str().map(String::from);
                }
            }
        }
    }
    None
}

/// Find the earliest deadline in the pending map.
fn earliest_deadline(pending: &HashMap<String, Instant>) -> Option<Instant> {
    pending.values().min().copied()
}

/// Sleep until a deadline, or sleep for a long time if no deadline.
async fn sleep_until_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => tokio::time::sleep(std::time::Duration::from_secs(3600)).await,
    }
}

/// Collect and remove all expired entries from the pending map.
fn collect_expired(pending: &mut HashMap<String, Instant>) -> Vec<String> {
    let now = Instant::now();
    let expired: Vec<String> = pending
        .iter()
        .filter(|(_, deadline)| **deadline <= now)
        .map(|(name, _)| name.clone())
        .collect();
    for name in &expired {
        pending.remove(name);
    }
    expired
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        invocable_wasm_bytes, test_manifest_toml, versioned_manifest_toml, write_test_skill,
        write_versioned_test_skill,
    };
    use crate::wasm_skill::load_wasm_artifact_from_dir;
    use crate::{SignaturePolicy, SkillLifecycleConfig, SkillLifecycleManager, SkillRegistry};
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn new_lifecycle(
        skills_dir: &Path,
        registry: Arc<SkillRegistry>,
    ) -> Arc<std::sync::Mutex<SkillLifecycleManager>> {
        Arc::new(std::sync::Mutex::new(SkillLifecycleManager::new(
            SkillLifecycleConfig {
                skills_dir: skills_dir.to_path_buf(),
                registry,
                credential_provider: None,
                signature_policy: SignaturePolicy::default(),
            },
        )))
    }

    fn new_watcher(
        skills_dir: &Path,
    ) -> (
        Arc<SkillRegistry>,
        Arc<std::sync::Mutex<SkillLifecycleManager>>,
        SkillWatcher,
        mpsc::Receiver<ReloadEvent>,
    ) {
        let registry = Arc::new(SkillRegistry::new());
        let lifecycle = new_lifecycle(skills_dir, Arc::clone(&registry));
        let (tx, rx) = mpsc::channel(16);
        let watcher = SkillWatcher::new(skills_dir.to_path_buf(), Arc::clone(&lifecycle), tx);
        (registry, lifecycle, watcher, rx)
    }

    fn load_startup_skills(lifecycle: &Arc<std::sync::Mutex<SkillLifecycleManager>>) {
        lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .load_startup_skills()
            .expect("load startup skills");
    }

    #[test]
    fn reload_event_is_debug_and_clone() {
        let event = ReloadEvent::Loaded {
            skill_name: "test".to_string(),
            version: "1.0.0".to_string(),
            revision: "abc123".to_string(),
            source: "installed".to_string(),
        };
        let cloned = event.clone();
        let _debug = format!("{event:?}");
        let _debug2 = format!("{cloned:?}");
    }

    #[test]
    fn extract_skill_dir_name_from_wasm_path() {
        let path = Path::new("/home/user/.fawx/skills/github/github.wasm");
        assert_eq!(extract_skill_dir_name(path), Some("github".to_string()));
    }

    #[test]
    fn extract_skill_dir_name_from_manifest_path() {
        let path = Path::new("/home/user/.fawx/skills/weather/manifest.toml");
        assert_eq!(extract_skill_dir_name(path), Some("weather".to_string()));
    }

    #[test]
    fn extract_skill_dir_name_from_dir_path() {
        let path = Path::new("/home/user/.fawx/skills/github");
        assert_eq!(extract_skill_dir_name(path), Some("github".to_string()));
    }

    #[test]
    fn extract_skill_dir_name_no_skills_component() {
        let path = Path::new("/home/user/something/github.wasm");
        assert_eq!(extract_skill_dir_name(path), None);
    }

    #[test]
    fn skill_dir_is_valid_with_all_files() {
        let tmp = TempDir::new().unwrap();
        let name = "test_skill";
        write_test_skill(tmp.path(), name).unwrap();
        assert!(skill_dir_is_valid(&tmp.path().join(name), name));
    }

    #[test]
    fn skill_dir_is_invalid_missing_manifest() {
        let tmp = TempDir::new().unwrap();
        let name = "test_skill";
        let skill_dir = tmp.path().join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join(format!("{name}.wasm")),
            invocable_wasm_bytes(),
        )
        .unwrap();
        assert!(!skill_dir_is_valid(&skill_dir, name));
    }

    #[test]
    fn skill_dir_is_invalid_missing_wasm() {
        let tmp = TempDir::new().unwrap();
        let name = "test_skill";
        let skill_dir = tmp.path().join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("manifest.toml"), test_manifest_toml(name)).unwrap();
        assert!(!skill_dir_is_valid(&skill_dir, name));
    }

    #[test]
    fn skill_dir_is_invalid_nonexistent() {
        let tmp = TempDir::new().unwrap();
        assert!(!skill_dir_is_valid(&tmp.path().join("nope"), "nope"));
    }

    #[test]
    fn initialize_hashes_populates_from_existing_skills() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "alpha").unwrap();
        write_test_skill(tmp.path(), "beta").unwrap();

        let (_registry, lifecycle, mut watcher, _rx) = new_watcher(tmp.path());
        load_startup_skills(&lifecycle);

        watcher.initialize_hashes();
        assert_eq!(watcher.hashes.len(), 2);
        assert!(watcher.hashes.contains_key("alpha"));
        assert!(watcher.hashes.contains_key("beta"));
    }

    #[test]
    fn initialize_hashes_correct_hash_value() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "test_hash").unwrap();

        let (_registry, lifecycle, mut watcher, _rx) = new_watcher(tmp.path());
        load_startup_skills(&lifecycle);

        watcher.initialize_hashes();

        let expected = load_wasm_artifact_from_dir(
            &tmp.path().join("test_hash"),
            None,
            &SignaturePolicy::default(),
        )
        .unwrap()
        .revision
        .revision_hash();
        assert_eq!(watcher.hashes.get("test_hash").unwrap().hash, expected);
    }

    #[test]
    fn initialize_hashes_stores_version() {
        let tmp = TempDir::new().unwrap();
        write_versioned_test_skill(tmp.path(), "versioned", "2.5.0").unwrap();

        let (_registry, lifecycle, mut watcher, _rx) = new_watcher(tmp.path());
        load_startup_skills(&lifecycle);

        watcher.initialize_hashes();
        assert_eq!(watcher.hashes.get("versioned").unwrap().version, "2.5.0");
    }

    #[test]
    fn initialize_hashes_uses_reconciled_offline_revision_after_restart() {
        let tmp = TempDir::new().unwrap();
        write_versioned_test_skill(tmp.path(), "weather", "1.0.0").unwrap();

        let registry = Arc::new(SkillRegistry::new());
        let lifecycle = new_lifecycle(tmp.path(), Arc::clone(&registry));
        load_startup_skills(&lifecycle);

        fs::write(
            tmp.path().join("weather").join("manifest.toml"),
            versioned_manifest_toml("weather", "2.0.0"),
        )
        .unwrap();

        let (_registry, lifecycle, mut watcher, _rx) = new_watcher(tmp.path());
        load_startup_skills(&lifecycle);

        watcher.initialize_hashes();
        assert_eq!(watcher.hashes.get("weather").unwrap().version, "2.0.0");
    }

    #[test]
    fn collect_expired_removes_past_deadlines() {
        let mut pending = HashMap::new();
        pending.insert(
            "expired".to_string(),
            Instant::now() - std::time::Duration::from_secs(1),
        );
        pending.insert(
            "future".to_string(),
            Instant::now() + std::time::Duration::from_secs(60),
        );

        let expired = collect_expired(&mut pending);
        assert_eq!(expired, vec!["expired".to_string()]);
        assert_eq!(pending.len(), 1);
        assert!(pending.contains_key("future"));
    }

    #[test]
    fn earliest_deadline_returns_minimum() {
        let mut pending = HashMap::new();
        let early = Instant::now();
        let late = early + std::time::Duration::from_secs(10);
        pending.insert("a".to_string(), late);
        pending.insert("b".to_string(), early);

        assert_eq!(earliest_deadline(&pending), Some(early));
    }

    #[test]
    fn earliest_deadline_empty_returns_none() {
        let pending: HashMap<String, Instant> = HashMap::new();
        assert_eq!(earliest_deadline(&pending), None);
    }

    #[tokio::test]
    async fn process_skill_change_loads_new_skill() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "newskill").unwrap();

        let (registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        watcher.process_skill_change("newskill").await;

        // Skill should be registered
        let defs = registry.all_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "newskill");

        // Hash should be stored
        assert!(watcher.hashes.contains_key("newskill"));

        // Event should be emitted
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ReloadEvent::Loaded { .. }));
    }

    #[tokio::test]
    async fn process_skill_change_loads_with_correct_version() {
        let tmp = TempDir::new().unwrap();
        write_versioned_test_skill(tmp.path(), "verskill", "3.1.0").unwrap();

        let (_registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        watcher.process_skill_change("verskill").await;

        let event = rx.try_recv().unwrap();
        match event {
            ReloadEvent::Loaded { version, .. } => assert_eq!(version, "3.1.0"),
            other => panic!("expected Loaded, got {other:?}"),
        }
        assert_eq!(watcher.hashes.get("verskill").unwrap().version, "3.1.0");
    }

    /// WAT source producing a different WASM binary (outputs "hi" instead of "ok").
    fn alternative_wasm_bytes() -> Vec<u8> {
        let wat = r#"
            (module
                (import "host_api_v1" "log" (func $log (param i32 i32 i32)))
                (import "host_api_v1" "kv_get" (func $kv_get (param i32 i32) (result i32)))
                (import "host_api_v1" "kv_set" (func $kv_set (param i32 i32 i32 i32)))
                (import "host_api_v1" "get_input" (func $get_input (result i32)))
                (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
                (memory (export "memory") 1)
                (func (export "run")
                    (i32.store8 (i32.const 0) (i32.const 104))
                    (i32.store8 (i32.const 1) (i32.const 105))
                    (call $set_output (i32.const 0) (i32.const 2))
                )
            )
        "#;
        wat.as_bytes().to_vec()
    }

    #[tokio::test]
    async fn process_skill_change_updates_existing_skill() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "updskill").unwrap();

        let (_registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        // First load
        watcher.process_skill_change("updskill").await;
        let _ = rx.try_recv(); // drain Loaded event

        // Write a different valid WASM to get a different hash
        let skill_dir = tmp.path().join("updskill");
        fs::write(skill_dir.join("updskill.wasm"), alternative_wasm_bytes()).unwrap();

        // Process again — should be an update
        watcher.process_skill_change("updskill").await;

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ReloadEvent::Updated { .. }));
    }

    #[tokio::test]
    async fn process_skill_change_update_reports_old_version() {
        let tmp = TempDir::new().unwrap();
        write_versioned_test_skill(tmp.path(), "upver", "1.0.0").unwrap();

        let (_registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        // First load
        watcher.process_skill_change("upver").await;
        let _ = rx.try_recv();

        // Update with new version and different wasm
        let skill_dir = tmp.path().join("upver");
        fs::write(
            skill_dir.join("manifest.toml"),
            versioned_manifest_toml("upver", "2.0.0"),
        )
        .unwrap();
        fs::write(skill_dir.join("upver.wasm"), alternative_wasm_bytes()).unwrap();

        watcher.process_skill_change("upver").await;

        let event = rx.try_recv().unwrap();
        match event {
            ReloadEvent::Updated {
                old_version,
                new_version,
                ..
            } => {
                assert_eq!(old_version, "1.0.0");
                assert_eq!(new_version, "2.0.0");
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_skill_change_manifest_only_update_changes_revision_identity() {
        let tmp = TempDir::new().unwrap();
        write_versioned_test_skill(tmp.path(), "manifestonly", "1.0.0").unwrap();

        let (_registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        watcher.process_skill_change("manifestonly").await;
        let _ = rx.try_recv();
        let old_hash = watcher.hashes.get("manifestonly").unwrap().hash.clone();

        fs::write(
            tmp.path().join("manifestonly").join("manifest.toml"),
            versioned_manifest_toml("manifestonly", "2.0.0"),
        )
        .unwrap();

        watcher.process_skill_change("manifestonly").await;

        let event = rx.try_recv().unwrap();
        match event {
            ReloadEvent::Updated {
                new_version,
                revision,
                ..
            } => {
                assert_eq!(new_version, "2.0.0");
                assert_ne!(revision, old_hash);
            }
            other => panic!("expected Updated, got {other:?}"),
        }
        assert_eq!(watcher.hashes.get("manifestonly").unwrap().version, "2.0.0");
        assert_ne!(watcher.hashes.get("manifestonly").unwrap().hash, old_hash);
    }

    #[tokio::test]
    async fn process_skill_change_same_hash_no_reload() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "sameskill").unwrap();

        let (_registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        // First load
        watcher.process_skill_change("sameskill").await;
        let _ = rx.try_recv(); // drain Loaded event

        // Process again without changing anything — hash unchanged
        watcher.process_skill_change("sameskill").await;

        // No event should be emitted
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn process_skill_change_removal() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "rmskill").unwrap();

        let (registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        // Load first
        watcher.process_skill_change("rmskill").await;
        let _ = rx.try_recv();

        // Remove the skill directory
        fs::remove_dir_all(tmp.path().join("rmskill")).unwrap();

        // Process — should be removal
        watcher.process_skill_change("rmskill").await;

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ReloadEvent::Removed { .. }));
        assert!(registry.all_tool_definitions().is_empty());
        assert!(!watcher.hashes.contains_key("rmskill"));
    }

    #[tokio::test]
    async fn process_skill_change_error_keeps_existing() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "errskill").unwrap();

        let (registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        // Load successfully first
        watcher.process_skill_change("errskill").await;
        let _ = rx.try_recv();
        let old_hash = watcher.hashes.get("errskill").unwrap().hash.clone();

        // Write invalid WASM but keep manifest valid
        let skill_dir = tmp.path().join("errskill");
        fs::write(skill_dir.join("errskill.wasm"), b"not valid wasm").unwrap();

        // Process — should emit error, keep old skill
        watcher.process_skill_change("errskill").await;

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ReloadEvent::Error { .. }));

        // Old hash should still be there (skill preserved)
        assert_eq!(watcher.hashes.get("errskill").unwrap().hash, old_hash);

        // Old skill should still be registered
        assert_eq!(registry.all_tool_definitions().len(), 1);
    }

    #[tokio::test]
    async fn process_skill_change_missing_manifest_error() {
        let tmp = TempDir::new().unwrap();
        let name = "nomanifest";
        let skill_dir = tmp.path().join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join(format!("{name}.wasm")),
            invocable_wasm_bytes(),
        )
        .unwrap();
        // No manifest.toml

        let (_registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        watcher.process_skill_change(name).await;

        // Should not load (no manifest = invalid dir, treated as removal/no-op)
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn debounce_multiple_events_single_reload() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "debounce").unwrap();

        let (_registry, _lifecycle, mut watcher, mut rx) = new_watcher(tmp.path());

        watcher.process_skill_change("debounce").await;
        let _ = rx.try_recv(); // Loaded

        // Same content again — should be no-op (hash unchanged)
        watcher.process_skill_change("debounce").await;
        assert!(
            rx.try_recv().is_err(),
            "should not emit event for unchanged hash"
        );
    }

    #[test]
    fn collect_skill_names_deduplicates() {
        let event = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![
                PathBuf::from("/home/user/.fawx/skills/github/github.wasm"),
                PathBuf::from("/home/user/.fawx/skills/github/manifest.toml"),
            ],
            attrs: Default::default(),
        };

        let mut names = HashSet::new();
        collect_skill_names_from_event(&event, &mut names);
        assert_eq!(names.len(), 1);
        assert!(names.contains("github"));
    }

    #[test]
    fn collect_skill_names_ignores_access_events() {
        let event = notify::Event {
            kind: EventKind::Access(notify::event::AccessKind::Read),
            paths: vec![PathBuf::from("/home/user/.fawx/skills/github/github.wasm")],
            attrs: Default::default(),
        };

        let mut names = HashSet::new();
        collect_skill_names_from_event(&event, &mut names);
        assert!(names.is_empty());
    }

    #[test]
    fn watcher_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<SkillWatcher>();
    }

    #[tokio::test]
    async fn handle_removal_uses_try_send() {
        let tmp = TempDir::new().unwrap();
        write_test_skill(tmp.path(), "trysend").unwrap();

        let registry = Arc::new(SkillRegistry::new());
        let lifecycle = new_lifecycle(tmp.path(), Arc::clone(&registry));
        // Channel with capacity 1 — fill it to verify try_send doesn't block
        let (tx, _rx) = mpsc::channel(1);
        let mut watcher = SkillWatcher::new(tmp.path().to_path_buf(), lifecycle, tx);

        // Load the skill first
        watcher.process_skill_change("trysend").await;

        // Fill the channel by processing another change (error event)
        let skill_dir = tmp.path().join("trysend");
        fs::write(skill_dir.join("trysend.wasm"), b"bad").unwrap();
        watcher.process_skill_change("trysend").await;

        // Now remove — try_send should not block even with full channel
        fs::remove_dir_all(tmp.path().join("trysend")).unwrap();
        watcher.process_skill_change("trysend").await;
        // If this completes without hanging, try_send works correctly
    }
}
