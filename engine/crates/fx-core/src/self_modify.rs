//! Path enforcement for agent self-modification.
//!
//! Provides a three-tier classification system (Allow / Propose / Deny)
//! that controls which files the agent may modify directly, which require
//! a proposal, and which are unconditionally blocked.

use std::fs;
use std::path::{Path, PathBuf};

/// Classification tier for a file path under self-modification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathTier {
    /// Agent may modify the path directly.
    Allow,
    /// Agent must create a proposal for this path.
    Propose,
    /// Agent is unconditionally blocked from modifying this path.
    Deny,
}

/// Default deny patterns shared between core and CLI configs.
pub const DEFAULT_DENY_PATHS: &[&str] = &[".git/**", "*.key", "*.pem", "credentials.*"];

/// Paths that always require proposal+approval, regardless of `self_modify.enabled`.
/// These are security-sensitive data files that the agent should never modify freely.
///
/// `*.key` and `*.pem` intentionally overlap with `DEFAULT_DENY_PATHS`: when
/// self-modify is enabled, always-propose wins so those files become human-gated
/// proposals instead of unconditional denies. That softer gate is intentional.
const ALWAYS_PROPOSE_PATTERNS: &[&str] = &[
    "config.toml",
    "credentials.db",
    "auth.db",
    "*.key",
    "*.pem",
    ".auth-salt",
    ".credentials-salt",
    ".bearer-token-ref",
];

/// Default proposals directory: `$HOME/.fawx/proposals`.
///
/// Falls back to `.fawx/proposals` (relative) when `HOME` is unset.
#[must_use]
pub fn default_proposals_dir() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(".fawx").join("proposals"),
        Err(_) => PathBuf::from(".fawx").join("proposals"),
    }
}

/// Configuration for the self-modification path enforcement system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfModifyConfig {
    pub enabled: bool,
    pub branch_prefix: String,
    pub require_tests: bool,
    pub allow_paths: Vec<String>,
    pub propose_paths: Vec<String>,
    pub deny_paths: Vec<String>,
    pub proposals_dir: PathBuf,
}

impl Default for SelfModifyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            branch_prefix: "fawx/improve".to_string(),
            require_tests: true,
            allow_paths: Vec::new(),
            propose_paths: Vec::new(),
            deny_paths: DEFAULT_DENY_PATHS.iter().map(|s| s.to_string()).collect(),
            proposals_dir: default_proposals_dir(),
        }
    }
}

/// Classify a path according to the self-modification policy.
///
/// `base_dir` is the working directory / repository root. The path is
/// made relative to `base_dir` before matching against glob patterns so
/// that absolute paths match patterns written as relative (e.g. `.git/**`).
///
/// When the policy is disabled (`!config.enabled`), security-sensitive
/// paths still require proposals while everything else remains
/// [`PathTier::Allow`] for backward compatibility.
///
/// Precedence: deny wins over propose/allow; propose wins over allow;
/// unknown paths default to [`PathTier::Deny`].
#[must_use]
pub fn classify_path(path: &Path, base_dir: &Path, config: &SelfModifyConfig) -> PathTier {
    let normalized = normalize_for_classification(path, base_dir);
    let filename = normalized
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if matches_always_propose(&normalized, filename) {
        return PathTier::Propose;
    }
    if !config.enabled {
        return PathTier::Allow;
    }
    if matches_any(&normalized, filename, &config.deny_paths) {
        return PathTier::Deny;
    }
    if matches_any(&normalized, filename, &config.propose_paths) {
        return PathTier::Propose;
    }
    if matches_any(&normalized, filename, &config.allow_paths) {
        return PathTier::Allow;
    }
    PathTier::Deny
}

/// Format a consistent self-modification policy violation error message.
#[must_use]
pub fn format_tier_violation(path: &Path, tier: PathTier) -> Option<String> {
    let path_display = path.display();
    match tier {
        PathTier::Allow => None,
        PathTier::Deny => Some(format!(
            "Self-modify policy violation [deny]: {path_display}. This path cannot be modified directly."
        )),
        PathTier::Propose => Some(format!(
            "Self-modify policy violation [propose]: {path_display}. Use the proposal system to request this change."
        )),
    }
}

/// Validate that all glob patterns in the config are syntactically valid.
///
/// Call this at config load time to fail fast on invalid patterns.
pub fn validate_glob_patterns(config: &SelfModifyConfig) -> Result<(), String> {
    let all_fields = [
        ("paths.allow", &config.allow_paths),
        ("paths.propose", &config.propose_paths),
        ("paths.deny", &config.deny_paths),
    ];
    for (field, patterns) in all_fields {
        for pattern in patterns {
            glob::Pattern::new(pattern).map_err(|error| {
                format!("invalid glob in self_modify.{field}: '{pattern}': {error}")
            })?;
        }
    }
    Ok(())
}

/// Match behavior is intentionally dual-mode:
/// - exact suffix match for literal path patterns (e.g. `src/lib.rs`)
/// - glob match for wildcard patterns (e.g. `src/**`, `*.rs`)
///
/// Glob matching runs against both the normalized full path and filename so
/// basename rules such as `*.key` work regardless of directory depth.
fn matches_any(path: &Path, filename: &str, patterns: &[String]) -> bool {
    let path_str = path.to_string_lossy();
    patterns.iter().any(|pattern| {
        matches_literal_suffix(path, filename, pattern)
            || matches_glob(&path_str, filename, pattern)
    })
}

fn matches_always_propose(path: &Path, filename: &str) -> bool {
    let path_str = path.to_string_lossy();
    ALWAYS_PROPOSE_PATTERNS.iter().any(|pattern| {
        matches_literal_suffix(path, filename, pattern)
            || matches_glob(&path_str, filename, pattern)
    })
}

fn matches_literal_suffix(path: &Path, filename: &str, pattern: &str) -> bool {
    path.ends_with(pattern) || (!filename.is_empty() && filename == pattern)
}

fn matches_glob(path_str: &str, filename: &str, pattern: &str) -> bool {
    let Ok(glob) = glob::Pattern::new(pattern) else {
        return false;
    };
    glob.matches(path_str) || (!filename.is_empty() && glob.matches(filename))
}

fn normalize_for_classification(path: &Path, base_dir: &Path) -> PathBuf {
    let absolute_path = as_absolute(path, base_dir);
    // Security requirement: when the target exists, canonicalize first so
    // symlinks are resolved before tier checks. For not-yet-created paths,
    // canonicalize cannot succeed, so we fall back to lexical `..` collapse.
    let normalized_path = if absolute_path.exists() {
        fs::canonicalize(&absolute_path).unwrap_or_else(|_| collapse_dot_dot(&absolute_path))
    } else {
        collapse_dot_dot(&absolute_path)
    };
    relativize_to_base(&normalized_path, base_dir)
}

fn relativize_to_base(path: &Path, base_dir: &Path) -> PathBuf {
    let lexical_base = collapse_dot_dot(base_dir);
    if let Ok(relative) = path.strip_prefix(&lexical_base) {
        return relative.to_path_buf();
    }

    if let Ok(canonical_base) = fs::canonicalize(base_dir) {
        if let Ok(relative) = path.strip_prefix(&canonical_base) {
            return relative.to_path_buf();
        }
    }

    path.to_path_buf()
}

fn as_absolute(path: &Path, base_dir: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

/// Best-effort `..` collapse for path normalization.
///
/// Uses `PathBuf` accumulation to correctly handle both absolute and
/// relative paths (avoids the double-slash bug from string joining).
fn collapse_dot_dot(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "fx-core-self-modify-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("temporary test directory should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn enabled_config() -> SelfModifyConfig {
        SelfModifyConfig {
            enabled: true,
            ..SelfModifyConfig::default()
        }
    }

    fn assert_path_tier(path: &str, enabled: bool, expected: PathTier) {
        let config = if enabled {
            enabled_config()
        } else {
            SelfModifyConfig::default()
        };
        let tier = classify_path(Path::new(path), Path::new(""), &config);
        assert_eq!(tier, expected);
    }

    #[test]
    fn classify_allow_path() {
        let mut config = enabled_config();
        config.allow_paths = vec!["src/**".to_string()];
        let tier = classify_path(Path::new("src/main.rs"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Allow);
    }

    #[test]
    fn classify_deny_path() {
        let config = enabled_config();
        let tier = classify_path(Path::new("credentials.json"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Deny);
    }

    #[test]
    fn classify_propose_path() {
        let mut config = enabled_config();
        config.propose_paths = vec!["kernel/**".to_string()];
        let tier = classify_path(Path::new("kernel/loop.rs"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_always_propose_wins_over_allow_and_default_deny() {
        let mut config = enabled_config();
        config.allow_paths = vec!["*.key".to_string()];
        let tier = classify_path(Path::new("server.key"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_config_toml_proposes_even_when_explicitly_allowed() {
        let mut config = enabled_config();
        config.allow_paths = vec!["config.toml".to_string()];
        let tier = classify_path(Path::new("config.toml"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_default_deny_unknown() {
        let config = enabled_config();
        let tier = classify_path(Path::new("random/unknown.txt"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Deny);
    }

    #[test]
    fn classify_disabled_returns_allow_for_normal_files() {
        let config = SelfModifyConfig::default(); // enabled=false
        let tier = classify_path(Path::new("docs/readme.md"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Allow);
    }

    #[test]
    fn classify_disabled_config_toml_returns_propose() {
        let config = SelfModifyConfig::default();
        let tier = classify_path(Path::new("config.toml"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_credentials_db_returns_propose() {
        let config = SelfModifyConfig::default();
        let tier = classify_path(Path::new("credentials.db"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_auth_db_returns_propose() {
        let config = SelfModifyConfig::default();
        let tier = classify_path(Path::new("auth.db"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_enabled_credentials_db_returns_propose() {
        assert_path_tier("credentials.db", true, PathTier::Propose);
    }

    #[test]
    fn classify_enabled_auth_db_returns_propose() {
        assert_path_tier("auth.db", true, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_auth_salt_returns_propose() {
        assert_path_tier(".auth-salt", false, PathTier::Propose);
    }

    #[test]
    fn classify_enabled_auth_salt_returns_propose() {
        assert_path_tier(".auth-salt", true, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_credentials_salt_returns_propose() {
        assert_path_tier(".credentials-salt", false, PathTier::Propose);
    }

    #[test]
    fn classify_enabled_credentials_salt_returns_propose() {
        assert_path_tier(".credentials-salt", true, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_bearer_token_ref_returns_propose() {
        assert_path_tier(".bearer-token-ref", false, PathTier::Propose);
    }

    #[test]
    fn classify_enabled_bearer_token_ref_returns_propose() {
        assert_path_tier(".bearer-token-ref", true, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_key_file_returns_propose() {
        let config = SelfModifyConfig::default();
        let tier = classify_path(Path::new("keys/server.key"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_pem_file_returns_propose() {
        let config = SelfModifyConfig::default();
        let tier = classify_path(Path::new("certs/server.pem"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_disabled_absolute_fawx_config_returns_propose() {
        let config = SelfModifyConfig::default();
        let base = PathBuf::from("/home/test/.fawx");
        let absolute = base.join("config.toml");
        let tier = classify_path(&absolute, &base, &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_dot_dot_traversal() {
        let mut config = enabled_config();
        config.allow_paths = vec!["src/**".to_string()];
        // "src/subdir/../main.rs" collapses to "src/main.rs"
        let tier = classify_path(
            &PathBuf::from("src/subdir/../main.rs"),
            Path::new(""),
            &config,
        );
        assert_eq!(tier, PathTier::Allow);
    }

    #[test]
    fn classify_nonexistent_path_uses_dot_dot_fallback() {
        let temp = TempDirGuard::new("nonexistent-fallback");
        let mut config = enabled_config();
        config.allow_paths = vec!["src/**".to_string()];
        let tier = classify_path(Path::new("src/new/../lib.rs"), temp.path(), &config);
        assert_eq!(tier, PathTier::Allow);
    }

    #[cfg(unix)]
    #[test]
    fn classify_resolves_symlink_targets_before_matching() {
        use std::os::unix::fs::symlink;

        let temp = TempDirGuard::new("symlink-resolution");
        let git_dir = temp.path().join(".git");
        fs::create_dir_all(&git_dir).expect("test .git directory should be created");

        let target = git_dir.join("config");
        fs::write(&target, "[core]\nrepoformatversion = 0\n")
            .expect("test .git/config should be created");

        let symlink_path = temp.path().join("linked-config");
        symlink(&target, &symlink_path).expect("symlink should be created");

        let config = enabled_config();
        let tier = classify_path(&symlink_path, temp.path(), &config);
        assert_eq!(tier, PathTier::Deny);
    }

    #[cfg(unix)]
    #[test]
    fn classify_nonexistent_path_handles_canonicalized_parent_paths() {
        use std::os::unix::fs::symlink;

        let temp = TempDirGuard::new("canonicalized-parent");
        let real_base = temp.path().join("real-base");
        fs::create_dir_all(&real_base).expect("real base should be created");

        let symlink_base = temp.path().join("symlink-base");
        symlink(&real_base, &symlink_base).expect("base symlink should be created");

        let canonical_base = fs::canonicalize(&symlink_base).expect("symlink base should resolve");
        let mut config = enabled_config();
        config.propose_paths = vec!["kernel/**".to_string()];

        let proposed_path = canonical_base.join("kernel/loop.rs");
        let tier = classify_path(&proposed_path, &symlink_base, &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_git_dir_denied() {
        let config = enabled_config();
        let tier = classify_path(Path::new(".git/config"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Deny);
    }

    #[test]
    fn classify_pem_file_requires_proposal() {
        let config = enabled_config();
        let tier = classify_path(Path::new("certs/server.pem"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn classify_credentials_file_denied() {
        let config = enabled_config();
        let tier = classify_path(Path::new("credentials.json"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Deny);
    }

    #[test]
    fn classify_absolute_path_matches_deny() {
        let config = enabled_config();
        let base = PathBuf::from("/tmp/test-repo");
        let absolute = base.join(".git/config");
        let tier = classify_path(&absolute, &base, &config);
        assert_eq!(tier, PathTier::Deny);
    }

    #[test]
    fn classify_absolute_path_matches_allow() {
        let mut config = enabled_config();
        config.allow_paths = vec!["src/**".to_string()];
        let base = PathBuf::from("/home/user/project");
        let absolute = base.join("src/lib.rs");
        let tier = classify_path(&absolute, &base, &config);
        assert_eq!(tier, PathTier::Allow);
    }

    #[test]
    fn collapse_dot_dot_handles_absolute_paths() {
        let path = Path::new("/tmp/repo/file.txt");
        assert_eq!(collapse_dot_dot(path), PathBuf::from("/tmp/repo/file.txt"));
    }

    #[test]
    fn collapse_dot_dot_handles_parent_in_absolute() {
        let path = Path::new("/tmp/repo/subdir/../file.txt");
        assert_eq!(collapse_dot_dot(path), PathBuf::from("/tmp/repo/file.txt"));
    }

    #[test]
    fn format_tier_violation_messages_are_consistent() {
        let deny = format_tier_violation(Path::new("secret.key"), PathTier::Deny)
            .expect("deny should produce a message");
        let propose = format_tier_violation(Path::new("kernel/loop.rs"), PathTier::Propose)
            .expect("propose should produce a message");

        assert!(deny.starts_with("Self-modify policy violation [deny]:"));
        assert!(propose.starts_with("Self-modify policy violation [propose]:"));
        assert!(propose.contains("proposal system"));
    }

    #[test]
    fn validate_rejects_invalid_glob() {
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["[invalid".to_string()],
            ..SelfModifyConfig::default()
        };
        let result = validate_glob_patterns(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("[invalid"));
    }

    #[test]
    fn validate_accepts_valid_globs() {
        let config = SelfModifyConfig::default();
        let result = validate_glob_patterns(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn classify_propose_wins_over_allow() {
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            allow_paths: vec!["kernel/**".to_string()],
            ..SelfModifyConfig::default()
        };
        let tier = classify_path(Path::new("kernel/loop.rs"), Path::new(""), &config);
        assert_eq!(tier, PathTier::Propose);
    }

    #[test]
    fn self_modify_config_has_equality() {
        let a = SelfModifyConfig::default();
        let b = SelfModifyConfig::default();
        assert_eq!(a, b);
    }

    #[test]
    fn default_proposals_dir_ends_with_fawx_proposals() {
        let dir = default_proposals_dir();
        assert!(
            dir.ends_with(".fawx/proposals"),
            "expected path ending with .fawx/proposals, got: {}",
            dir.display()
        );
    }
}
