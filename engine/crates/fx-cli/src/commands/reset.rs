use super::runtime_layout::RuntimeLayout;
use crate::config_redaction::is_secret_key;
use anyhow::Context;
use clap::Args;
use fx_config::FawxConfig;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const CONVERSATIONS_DIR: &str = "conversations";
const SESSIONS_DB_FILE: &str = "sessions.redb";

#[derive(Debug, Clone, Args)]
pub struct ResetArgs {
    /// Clear persisted memory data and memory embeddings
    #[arg(long)]
    pub memory: bool,
    /// Clear saved conversations, session history, and persisted signals
    #[arg(long)]
    pub conversations: bool,
    /// Reset config to defaults while preserving credentials
    #[arg(long)]
    pub config: bool,
    /// Reset memory, conversations, and config while preserving credentials
    #[arg(long, conflicts_with_all = ["memory", "conversations", "config"])]
    pub all: bool,
    /// Skip the destructive-action confirmation prompt
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum ResetScope {
    Memory,
    Conversations,
    Config,
}

#[derive(Debug, Clone)]
enum ResetAction {
    RemovePath { label: &'static str, path: PathBuf },
    ResetConfig,
}

#[derive(Debug, Clone)]
struct ResetPlan {
    data_dir: PathBuf,
    config_path: PathBuf,
    current_config: FawxConfig,
    scopes: BTreeSet<ResetScope>,
    actions: Vec<ResetAction>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct ResetSummary {
    pub(crate) removed: Vec<String>,
    pub(crate) already_clean: Vec<String>,
    pub(crate) config_reset: bool,
    pub(crate) credentials_preserved: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ResetOutcome {
    Applied(ResetSummary),
    Cancelled,
}

pub fn run(args: &ResetArgs) -> anyhow::Result<i32> {
    let layout = RuntimeLayout::detect()?;
    match execute_with_confirmation(args, &layout, prompt_for_confirmation)? {
        ResetOutcome::Applied(summary) => {
            print_summary(&summary);
            Ok(0)
        }
        ResetOutcome::Cancelled => {
            println!("Reset cancelled.");
            Ok(1)
        }
    }
}

pub(crate) fn execute_with_confirmation<F>(
    args: &ResetArgs,
    layout: &RuntimeLayout,
    confirm: F,
) -> anyhow::Result<ResetOutcome>
where
    F: FnOnce(&str) -> anyhow::Result<bool>,
{
    let plan = build_plan(layout, args)?;
    if !args.force && !confirm(&confirmation_prompt(&plan))? {
        return Ok(ResetOutcome::Cancelled);
    }
    execute_plan(plan).map(ResetOutcome::Applied)
}

fn build_plan(layout: &RuntimeLayout, args: &ResetArgs) -> anyhow::Result<ResetPlan> {
    let scopes = selected_scopes(args)?;
    let actions = build_actions(layout, &scopes);
    validate_reset_root(&layout.data_dir)?;
    Ok(ResetPlan {
        data_dir: layout.data_dir.clone(),
        config_path: layout.config_path.clone(),
        current_config: layout.config.clone(),
        scopes,
        actions,
    })
}

fn selected_scopes(args: &ResetArgs) -> anyhow::Result<BTreeSet<ResetScope>> {
    let mut scopes = BTreeSet::new();
    if args.memory || args.all {
        scopes.insert(ResetScope::Memory);
    }
    if args.conversations || args.all {
        scopes.insert(ResetScope::Conversations);
    }
    if args.config || args.all {
        scopes.insert(ResetScope::Config);
    }
    if scopes.is_empty() {
        return Err(anyhow::anyhow!(
            "select at least one reset scope (--memory, --conversations, --config, or --all)"
        ));
    }
    Ok(scopes)
}

fn build_actions(layout: &RuntimeLayout, scopes: &BTreeSet<ResetScope>) -> Vec<ResetAction> {
    let mut actions = Vec::new();
    if scopes.contains(&ResetScope::Memory) {
        actions.extend(memory_actions(layout));
    }
    if scopes.contains(&ResetScope::Conversations) {
        actions.extend(conversation_actions(layout));
    }
    if scopes.contains(&ResetScope::Config) {
        actions.push(ResetAction::ResetConfig);
    }
    actions
}

fn memory_actions(layout: &RuntimeLayout) -> Vec<ResetAction> {
    vec![
        remove_path_action("memory", layout.data_dir.join("memory")),
        remove_path_action("memory embeddings", layout.embedding_model_dir.clone()),
    ]
}

fn conversation_actions(layout: &RuntimeLayout) -> Vec<ResetAction> {
    vec![
        remove_path_action("conversations", layout.data_dir.join(CONVERSATIONS_DIR)),
        remove_path_action("signals", layout.sessions_dir.clone()),
        remove_path_action("session database", layout.data_dir.join(SESSIONS_DB_FILE)),
    ]
}

fn remove_path_action(label: &'static str, path: PathBuf) -> ResetAction {
    ResetAction::RemovePath { label, path }
}

fn validate_reset_root(data_dir: &Path) -> anyhow::Result<()> {
    if data_dir.parent().is_none() {
        return Err(anyhow::anyhow!(
            "refusing to reset using filesystem root as the Fawx data directory"
        ));
    }
    Ok(())
}

fn confirmation_prompt(plan: &ResetPlan) -> String {
    let scopes = plan
        .scopes
        .iter()
        .map(reset_scope_label)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "This will reset {scopes} in {} while preserving credentials. Continue? [y/N]: ",
        plan.data_dir.display()
    )
}

fn reset_scope_label(scope: &ResetScope) -> &'static str {
    match scope {
        ResetScope::Memory => "memory",
        ResetScope::Conversations => "conversations",
        ResetScope::Config => "config",
    }
}

fn prompt_for_confirmation(prompt: &str) -> anyhow::Result<bool> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation")?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn execute_plan(plan: ResetPlan) -> anyhow::Result<ResetSummary> {
    let mut summary = ResetSummary {
        credentials_preserved: true,
        ..ResetSummary::default()
    };
    for action in plan.actions.clone() {
        apply_action(&plan, action, &mut summary)?;
    }
    Ok(summary)
}

fn apply_action(
    plan: &ResetPlan,
    action: ResetAction,
    summary: &mut ResetSummary,
) -> anyhow::Result<()> {
    match action {
        ResetAction::RemovePath { label, path } => {
            record_path_outcome(summary, label, remove_managed_path(&plan.data_dir, &path)?)
        }
        ResetAction::ResetConfig => reset_config(plan, summary),
    }
}

fn record_path_outcome(
    summary: &mut ResetSummary,
    label: &'static str,
    removed: bool,
) -> anyhow::Result<()> {
    if removed {
        summary.removed.push(label.to_string());
    } else {
        summary.already_clean.push(label.to_string());
    }
    Ok(())
}

fn remove_managed_path(data_dir: &Path, path: &Path) -> anyhow::Result<bool> {
    ensure_within_data_dir(data_dir, path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    remove_path_for_metadata(path, &metadata)?;
    Ok(true)
}

fn ensure_within_data_dir(data_dir: &Path, path: &Path) -> anyhow::Result<()> {
    if path.starts_with(data_dir) {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "refusing to reset unmanaged path: {}",
        path.display()
    ))
}

fn remove_path_for_metadata(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return fs::remove_file(path)
            .with_context(|| format!("failed to remove {}", path.display()));
    }
    fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))
}

fn reset_config(plan: &ResetPlan, summary: &mut ResetSummary) -> anyhow::Result<()> {
    let config = reset_config_defaults(&plan.current_config)?;
    write_reset_config(&plan.config_path, &config)?;
    summary.config_reset = true;
    Ok(())
}

fn write_reset_config(config_path: &Path, config: &FawxConfig) -> anyhow::Result<()> {
    let parent = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent directory"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to prepare {}", parent.display()))?;
    let content = toml::to_string_pretty(config).context("failed to serialize reset config")?;
    fs::write(config_path, content)
        .with_context(|| format!("failed to write {}", config_path.display()))
}

fn reset_config_defaults(current: &FawxConfig) -> anyhow::Result<FawxConfig> {
    let mut defaults = serde_json::to_value(FawxConfig::default())?;
    let current = serde_json::to_value(current)?;
    preserve_reset_values(&mut defaults, &current, None);
    serde_json::from_value(defaults).context("failed to build reset config")
}

fn preserve_reset_values(
    target: &mut serde_json::Value,
    source: &serde_json::Value,
    parent: Option<&str>,
) {
    match (target, source) {
        (serde_json::Value::Object(target_map), serde_json::Value::Object(source_map)) => {
            preserve_object_values(target_map, source_map, parent)
        }
        (serde_json::Value::Array(target_items), serde_json::Value::Array(source_items)) => {
            preserve_array_values(target_items, source_items, parent)
        }
        _ => {}
    }
}

fn preserve_object_values(
    target_map: &mut serde_json::Map<String, serde_json::Value>,
    source_map: &serde_json::Map<String, serde_json::Value>,
    parent: Option<&str>,
) {
    for (key, source_value) in source_map {
        if should_preserve_key(parent, key) {
            target_map.insert(key.clone(), source_value.clone());
            continue;
        }
        if let Some(target_value) = target_map.get_mut(key) {
            preserve_reset_values(target_value, source_value, Some(key));
            continue;
        }
        insert_missing_preserved_value(target_map, key, source_value);
    }
}

fn insert_missing_preserved_value(
    target_map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    source_value: &serde_json::Value,
) {
    if let Some(value) = build_preserved_reset_value(source_value, Some(key)) {
        target_map.insert(key.to_string(), value);
    }
}

fn preserve_array_values(
    target_items: &mut Vec<serde_json::Value>,
    source_items: &[serde_json::Value],
    parent: Option<&str>,
) {
    let existing_len = target_items.len();
    for (target_item, source_item) in target_items.iter_mut().zip(source_items) {
        preserve_reset_values(target_item, source_item, parent);
    }
    let missing_items = source_items
        .iter()
        .skip(existing_len)
        .filter_map(|source_item| build_preserved_reset_value(source_item, parent))
        .collect::<Vec<_>>();
    target_items.extend(missing_items);
}

fn build_preserved_reset_value(
    source: &serde_json::Value,
    parent: Option<&str>,
) -> Option<serde_json::Value> {
    if !subtree_contains_preserved_values(source, parent) {
        return None;
    }
    let mut target = reset_template_value_for_parent(source, parent);
    preserve_reset_values(&mut target, source, parent);
    Some(target)
}

fn reset_template_value_for_parent(
    value: &serde_json::Value,
    parent: Option<&str>,
) -> serde_json::Value {
    if parent == Some("nodes") && value.is_object() {
        return reset_fleet_node_template();
    }
    reset_template_value(value)
}

fn reset_template_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, child)| (key.clone(), reset_template_value(child)))
                .collect(),
        ),
        serde_json::Value::Array(_) => serde_json::Value::Array(Vec::new()),
        serde_json::Value::String(_) => serde_json::Value::String(String::new()),
        serde_json::Value::Number(number) => reset_number_value(number),
        serde_json::Value::Bool(_) => serde_json::Value::Bool(false),
        serde_json::Value::Null => serde_json::Value::Null,
    }
}

fn reset_number_value(number: &serde_json::Number) -> serde_json::Value {
    if number.is_f64() {
        serde_json::json!(0.0)
    } else {
        serde_json::json!(0)
    }
}

fn reset_fleet_node_template() -> serde_json::Value {
    serde_json::json!({
        "id": "",
        "name": "",
        "endpoint": null,
        "auth_token": null,
        "capabilities": [],
        "address": null,
        "user": null,
        "ssh_key": null,
    })
}

fn subtree_contains_preserved_values(value: &serde_json::Value, parent: Option<&str>) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, child)| {
            should_preserve_key(parent, key)
                || subtree_contains_preserved_values(child, Some(key.as_str()))
        }),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| subtree_contains_preserved_values(item, parent)),
        _ => false,
    }
}

fn should_preserve_key(parent: Option<&str>, key: &str) -> bool {
    is_secret_key(key) || matches!((parent, key), (Some("general"), "data_dir"))
}

fn print_summary(summary: &ResetSummary) {
    println!("Reset complete (credentials preserved).");
    print_summary_items("Removed", &summary.removed);
    print_summary_items("Already clean", &summary.already_clean);
    if summary.config_reset {
        println!("- Config reset to defaults");
    }
}

fn print_summary_items(label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    println!("- {label}: {}", items.join(", "));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct ResetFixture {
        _base: TempDir,
        _data: TempDir,
        layout: RuntimeLayout,
    }

    impl ResetFixture {
        fn new(config_toml: &str) -> Self {
            let base = TempDir::new().expect("base tempdir");
            let data = TempDir::new().expect("data tempdir");
            std::fs::write(base.path().join("config.toml"), config_toml).expect("write config");
            let config = FawxConfig::load(base.path()).expect("load config");
            Self {
                layout: RuntimeLayout {
                    data_dir: data.path().to_path_buf(),
                    config_path: base.path().join("config.toml"),
                    storage_dir: data.path().join("storage"),
                    audit_log_path: data.path().join("audit.log"),
                    auth_db_path: data.path().join("auth.db"),
                    logs_dir: data.path().join("logs"),
                    skills_dir: data.path().join("skills"),
                    trusted_keys_dir: data.path().join("trusted_keys"),
                    embedding_model_dir: data.path().join("models").join("embed"),
                    pid_file: data.path().join("fawx.pid"),
                    memory_json_path: data.path().join("memory").join("memory.json"),
                    sessions_dir: data.path().join("signals"),
                    security_baseline_path: data.path().join("security-baseline.json"),
                    repo_root: PathBuf::from("/tmp/fawx"),
                    http_port: 8400,
                    config,
                },
                _base: base,
                _data: data,
            }
        }
    }

    fn test_args() -> ResetArgs {
        ResetArgs {
            memory: false,
            conversations: false,
            config: false,
            all: false,
            force: false,
        }
    }

    fn write_file(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        std::fs::write(path, "data").expect("write file");
    }

    fn write_dir_file(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).expect("create dir");
        std::fs::write(dir.join(name), "data").expect("write child");
    }

    #[test]
    fn reset_requires_at_least_one_scope() {
        let fixture = ResetFixture::new("");
        let error = build_plan(&fixture.layout, &test_args()).expect_err("missing scope");
        assert!(error
            .to_string()
            .contains("select at least one reset scope"));
    }

    #[test]
    fn memory_reset_deletes_memory_entries_and_embeddings_only() {
        let fixture = ResetFixture::new("");
        write_dir_file(&fixture.layout.data_dir.join("memory"), "memory.json");
        write_dir_file(&fixture.layout.embedding_model_dir, "index.bin");
        write_file(&fixture.layout.auth_db_path);
        write_dir_file(
            &fixture.layout.data_dir.join(CONVERSATIONS_DIR),
            "conv.jsonl",
        );
        let args = ResetArgs {
            memory: true,
            ..test_args()
        };

        let outcome =
            execute_with_confirmation(&args, &fixture.layout, |_| Ok(true)).expect("reset outcome");

        assert!(matches!(outcome, ResetOutcome::Applied(_)));
        assert!(!fixture.layout.data_dir.join("memory").exists());
        assert!(!fixture.layout.embedding_model_dir.exists());
        assert!(fixture.layout.auth_db_path.exists());
        assert!(fixture.layout.data_dir.join(CONVERSATIONS_DIR).exists());
    }

    #[test]
    fn conversations_reset_deletes_only_conversation_state() {
        let fixture = ResetFixture::new("");
        write_dir_file(
            &fixture.layout.data_dir.join(CONVERSATIONS_DIR),
            "conv.jsonl",
        );
        write_dir_file(&fixture.layout.sessions_dir, "session.jsonl");
        write_file(&fixture.layout.data_dir.join(SESSIONS_DB_FILE));
        write_dir_file(&fixture.layout.data_dir.join("memory"), "memory.json");
        let args = ResetArgs {
            conversations: true,
            ..test_args()
        };

        execute_with_confirmation(&args, &fixture.layout, |_| Ok(true)).expect("reset outcome");

        assert!(!fixture.layout.data_dir.join(CONVERSATIONS_DIR).exists());
        assert!(!fixture.layout.sessions_dir.exists());
        assert!(!fixture.layout.data_dir.join(SESSIONS_DB_FILE).exists());
        assert!(fixture.layout.data_dir.join("memory").exists());
    }

    #[test]
    fn config_reset_preserves_credentials_and_data_dir() {
        let fixture = ResetFixture::new(
            "[general]\ndata_dir = \"/custom/data\"\n\n[model]\ndefault_model = \"custom\"\n\n[http]\nbearer_token = \"keep-me\"\n",
        );
        let args = ResetArgs {
            config: true,
            ..test_args()
        };

        execute_with_confirmation(&args, &fixture.layout, |_| Ok(true)).expect("reset outcome");

        let reset = FawxConfig::load(
            fixture
                .layout
                .config_path
                .parent()
                .expect("config parent directory"),
        )
        .expect("reload config");
        assert_eq!(reset.general.data_dir, Some(PathBuf::from("/custom/data")));
        assert_eq!(reset.http.bearer_token.as_deref(), Some("keep-me"));
        assert_eq!(
            reset.model.default_model,
            FawxConfig::default().model.default_model
        );
    }

    #[test]
    fn config_reset_preserves_only_array_backed_node_credentials() {
        let fixture = ResetFixture::new(
            "[fleet]\ncoordinator = true\nstale_timeout_seconds = 120\n\n[[fleet.nodes]]\nid = \"node-1\"\nname = \"Node One\"\nendpoint = \"https://node.example\"\nauth_token = \"keep-token\"\nssh_key = \"~/.ssh/node-1\"\ncapabilities = [\"agentic_loop\"]\n",
        );
        let args = ResetArgs {
            config: true,
            ..test_args()
        };

        execute_with_confirmation(&args, &fixture.layout, |_| Ok(true)).expect("reset outcome");

        let reset = FawxConfig::load(
            fixture
                .layout
                .config_path
                .parent()
                .expect("config parent directory"),
        )
        .expect("reload config");
        assert_eq!(
            reset.fleet.coordinator,
            FawxConfig::default().fleet.coordinator
        );
        assert_eq!(
            reset.fleet.stale_timeout_seconds,
            FawxConfig::default().fleet.stale_timeout_seconds
        );
        assert_eq!(reset.fleet.nodes.len(), 1);
        assert_eq!(reset.fleet.nodes[0].id, "");
        assert_eq!(reset.fleet.nodes[0].name, "");
        assert_eq!(reset.fleet.nodes[0].endpoint, None);
        assert!(reset.fleet.nodes[0].capabilities.is_empty());
        assert_eq!(reset.fleet.nodes[0].address, None);
        assert_eq!(reset.fleet.nodes[0].user, None);
        assert_eq!(
            reset.fleet.nodes[0].auth_token.as_deref(),
            Some("keep-token")
        );
        assert_eq!(
            reset.fleet.nodes[0].ssh_key.as_deref(),
            Some("~/.ssh/node-1")
        );
    }

    #[test]
    fn all_reset_preserves_credentials_while_resetting_the_rest() {
        let fixture = ResetFixture::new(
            "[http]\nbearer_token = \"keep-me\"\n\n[telegram]\nbot_token = \"keep-bot\"\n\n[[fleet.nodes]]\nid = \"node-1\"\nname = \"Node One\"\nendpoint = \"https://node.example\"\nauth_token = \"keep-token\"\nssh_key = \"~/.ssh/node-1\"\ncapabilities = [\"agentic_loop\"]\naddress = \"100.64.0.1\"\nuser = \"deploy\"\n",
        );
        write_dir_file(&fixture.layout.data_dir.join("memory"), "memory.json");
        write_dir_file(&fixture.layout.embedding_model_dir, "index.bin");
        write_dir_file(
            &fixture.layout.data_dir.join(CONVERSATIONS_DIR),
            "conv.jsonl",
        );
        write_dir_file(&fixture.layout.sessions_dir, "signal.jsonl");
        write_file(&fixture.layout.auth_db_path);
        let args = ResetArgs {
            all: true,
            ..test_args()
        };

        execute_with_confirmation(&args, &fixture.layout, |_| Ok(true)).expect("reset outcome");

        let reset = FawxConfig::load(
            fixture
                .layout
                .config_path
                .parent()
                .expect("config parent directory"),
        )
        .expect("reload config");
        assert!(!fixture.layout.data_dir.join("memory").exists());
        assert!(!fixture.layout.embedding_model_dir.exists());
        assert!(!fixture.layout.data_dir.join(CONVERSATIONS_DIR).exists());
        assert!(!fixture.layout.sessions_dir.exists());
        assert!(fixture.layout.auth_db_path.exists());
        assert_eq!(reset.http.bearer_token.as_deref(), Some("keep-me"));
        assert_eq!(reset.telegram.bot_token.as_deref(), Some("keep-bot"));
        assert_eq!(reset.fleet.nodes.len(), 1);
        assert_eq!(reset.fleet.nodes[0].id, "");
        assert_eq!(reset.fleet.nodes[0].name, "");
        assert_eq!(reset.fleet.nodes[0].endpoint, None);
        assert!(reset.fleet.nodes[0].capabilities.is_empty());
        assert_eq!(reset.fleet.nodes[0].address, None);
        assert_eq!(reset.fleet.nodes[0].user, None);
        assert_eq!(
            reset.fleet.nodes[0].auth_token.as_deref(),
            Some("keep-token")
        );
        assert_eq!(
            reset.fleet.nodes[0].ssh_key.as_deref(),
            Some("~/.ssh/node-1")
        );
    }

    #[test]
    fn missing_targets_are_treated_as_already_clean() {
        let fixture = ResetFixture::new("");
        let args = ResetArgs {
            conversations: true,
            ..test_args()
        };
        let outcome =
            execute_with_confirmation(&args, &fixture.layout, |_| Ok(true)).expect("reset outcome");
        assert_eq!(
            outcome,
            ResetOutcome::Applied(ResetSummary {
                removed: Vec::new(),
                already_clean: vec![
                    "conversations".to_string(),
                    "signals".to_string(),
                    "session database".to_string(),
                ],
                config_reset: false,
                credentials_preserved: true,
            })
        );
    }

    #[test]
    fn confirmation_is_required_unless_force_is_used() {
        let fixture = ResetFixture::new("");
        let mut prompted = false;
        let outcome = execute_with_confirmation(
            &ResetArgs {
                memory: true,
                ..test_args()
            },
            &fixture.layout,
            |_| {
                prompted = true;
                Ok(false)
            },
        )
        .expect("cancelled reset");
        assert!(prompted);
        assert_eq!(outcome, ResetOutcome::Cancelled);
    }

    #[test]
    fn force_skips_confirmation() {
        let fixture = ResetFixture::new("");
        let outcome = execute_with_confirmation(
            &ResetArgs {
                memory: true,
                force: true,
                ..test_args()
            },
            &fixture.layout,
            |_| Err(anyhow::anyhow!("should not prompt")),
        )
        .expect("forced reset");
        assert!(matches!(outcome, ResetOutcome::Applied(_)));
    }

    #[test]
    fn reset_plan_never_targets_auth_files() {
        let fixture = ResetFixture::new("");
        let plan = build_plan(
            &fixture.layout,
            &ResetArgs {
                all: true,
                ..test_args()
            },
        )
        .expect("build plan");
        let targeted = plan
            .actions
            .iter()
            .filter_map(|action| match action {
                ResetAction::RemovePath { path, .. } => Some(path.clone()),
                ResetAction::ResetConfig => None,
            })
            .collect::<Vec<_>>();
        assert!(!targeted.contains(&fixture.layout.auth_db_path));
        assert!(!targeted.iter().any(|path| path.ends_with(".auth-salt")));
    }
}
