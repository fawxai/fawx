use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A tripwire boundary within the capability space.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TripwireConfig {
    /// Unique identifier for this tripwire.
    pub id: String,
    /// What kind of boundary this represents.
    pub kind: TripwireKind,
    /// Human-readable description shown in notifications.
    pub description: String,
    /// Whether this tripwire is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// The type of boundary a tripwire monitors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TripwireKind {
    /// Matches file paths against a glob pattern.
    Path { pattern: String },
    /// Matches tool action category with optional command pattern.
    Action {
        category: String,
        #[serde(default)]
        pattern: Option<String>,
    },
    /// Fires after N actions of a category in one session.
    Threshold { category: String, min_count: u32 },
}

/// Default tripwires shipped with the Standard preset.
pub fn default_tripwires() -> Vec<TripwireConfig> {
    vec![
        TripwireConfig {
            id: "outside_project".into(),
            kind: TripwireKind::Path {
                pattern: "!{project_dir}/**".into(),
            },
            description: "Writes outside project directory".into(),
            enabled: true,
        },
        TripwireConfig {
            id: "credential_read".into(),
            kind: TripwireKind::Path {
                pattern: "~/.ssh/** | ~/.aws/** | ~/.gnupg/** | ~/.config/gh/**".into(),
            },
            description: "Credential file access".into(),
            enabled: true,
        },
        TripwireConfig {
            id: "git_push".into(),
            kind: TripwireKind::Action {
                category: "git".into(),
                pattern: Some("push".into()),
            },
            description: "Git push to remote".into(),
            enabled: true,
        },
        TripwireConfig {
            id: "bulk_delete".into(),
            kind: TripwireKind::Threshold {
                category: "file_delete".into(),
                min_count: 5,
            },
            description: "Bulk file deletion (5+ files)".into(),
            enabled: true,
        },
    ]
}

/// Replace `{project_dir}` placeholder in tripwire patterns with the actual project path.
pub fn resolve_tripwires(tripwires: &mut [TripwireConfig], project_dir: &str) {
    for tripwire in tripwires.iter_mut() {
        if let TripwireKind::Path { pattern } = &mut tripwire.kind {
            *pattern = pattern.replace("{project_dir}", project_dir);
        }
    }
}

impl TripwireConfig {
    /// Check if this tripwire matches an action.
    pub fn matches(
        &self,
        category: &str,
        path: Option<&str>,
        command: Option<&str>,
        session_category_counts: &HashMap<String, u32>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        match &self.kind {
            TripwireKind::Path { pattern } => {
                path.is_some_and(|value| path_matches_pattern(value, pattern))
            }
            TripwireKind::Action {
                category: tripwire_category,
                pattern,
            } => action_matches(category, command, tripwire_category, pattern.as_deref()),
            TripwireKind::Threshold {
                category: tripwire_category,
                min_count,
            } => threshold_matches(
                category,
                tripwire_category,
                *min_count,
                session_category_counts,
            ),
        }
    }
}

fn action_matches(
    category: &str,
    command: Option<&str>,
    tripwire_category: &str,
    pattern: Option<&str>,
) -> bool {
    if category != tripwire_category {
        return false;
    }

    match pattern {
        Some(value) => command.is_some_and(|text| text.contains(value)),
        None => true,
    }
}

fn threshold_matches(
    category: &str,
    tripwire_category: &str,
    min_count: u32,
    session_category_counts: &HashMap<String, u32>,
) -> bool {
    let count = session_category_counts
        .get(tripwire_category)
        .copied()
        .unwrap_or(0);
    category == tripwire_category && count >= min_count
}

/// Simple glob matching for tripwire path patterns.
/// Supports: `**` (recursive), `*` (single segment), `|` (alternatives).
/// Handles `~` expansion and `!` prefix (negation — matches paths NOT under pattern).
fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    pattern
        .split('|')
        .map(str::trim)
        .any(|alternative| match_alternative(path, alternative))
}

fn match_alternative(path: &str, alternative: &str) -> bool {
    if let Some(negated) = alternative.strip_prefix('!') {
        return !simple_glob(path, negated.trim());
    }
    simple_glob(path, alternative)
}

/// Minimal glob: `**` matches any path segments, `*` matches one segment.
fn simple_glob(path: &str, pattern: &str) -> bool {
    match pattern.split_once("**") {
        Some((prefix, _)) => path.starts_with(&expand_tilde(prefix)),
        None => path == expand_tilde(pattern),
    }
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

fn home_dir() -> Option<String> {
    std::env::var("HOME").ok()
}

#[cfg(test)]
mod tests {
    use super::{default_tripwires, resolve_tripwires, TripwireConfig, TripwireKind};
    use std::collections::HashMap;

    #[test]
    fn path_tripwire_matches_credential_path() {
        let config = TripwireConfig {
            id: "credential_read".into(),
            kind: TripwireKind::Path {
                pattern: "~/.ssh/** | ~/.aws/**".into(),
            },
            description: "Credential file access".into(),
            enabled: true,
        };
        let home = std::env::var("HOME").expect("HOME set for tests");
        let path = format!("{home}/.ssh/id_ed25519");

        assert!(config.matches("file_read", Some(&path), None, &HashMap::new()));
    }

    #[test]
    fn path_tripwire_ignores_unrelated_path() {
        let config = TripwireConfig {
            id: "credential_read".into(),
            kind: TripwireKind::Path {
                pattern: "~/.ssh/** | ~/.aws/**".into(),
            },
            description: "Credential file access".into(),
            enabled: true,
        };

        assert!(!config.matches(
            "file_read",
            Some("/tmp/unrelated.txt"),
            None,
            &HashMap::new(),
        ));
    }

    #[test]
    fn action_tripwire_matches_category_and_pattern() {
        let config = TripwireConfig {
            id: "git_push".into(),
            kind: TripwireKind::Action {
                category: "git".into(),
                pattern: Some("push".into()),
            },
            description: "Git push to remote".into(),
            enabled: true,
        };

        assert!(config.matches("git", None, Some("git push origin dev"), &HashMap::new(),));
    }

    #[test]
    fn action_tripwire_matches_category_without_pattern() {
        let config = TripwireConfig {
            id: "git_action".into(),
            kind: TripwireKind::Action {
                category: "git".into(),
                pattern: None,
            },
            description: "Any git action".into(),
            enabled: true,
        };

        assert!(config.matches("git", None, None, &HashMap::new()));
    }

    #[test]
    fn threshold_tripwire_fires_at_count() {
        let config = TripwireConfig {
            id: "bulk_delete".into(),
            kind: TripwireKind::Threshold {
                category: "file_delete".into(),
                min_count: 5,
            },
            description: "Bulk file deletion".into(),
            enabled: true,
        };
        let counts = HashMap::from([("file_delete".to_string(), 5)]);

        assert!(config.matches("file_delete", None, None, &counts));
    }

    #[test]
    fn threshold_tripwire_does_not_fire_below_count() {
        let config = TripwireConfig {
            id: "bulk_delete".into(),
            kind: TripwireKind::Threshold {
                category: "file_delete".into(),
                min_count: 5,
            },
            description: "Bulk file deletion".into(),
            enabled: true,
        };
        let counts = HashMap::from([("file_delete".to_string(), 4)]);

        assert!(!config.matches("file_delete", None, None, &counts));
    }

    #[test]
    fn disabled_tripwire_never_matches() {
        let config = TripwireConfig {
            id: "git_push".into(),
            kind: TripwireKind::Action {
                category: "git".into(),
                pattern: Some("push".into()),
            },
            description: "Git push to remote".into(),
            enabled: false,
        };

        assert!(!config.matches(
            "git",
            Some("/tmp/file.txt"),
            Some("git push origin dev"),
            &HashMap::from([("git".to_string(), 99)]),
        ));
    }

    #[test]
    fn default_tripwires_returns_four_entries() {
        assert_eq!(default_tripwires().len(), 4);
    }

    #[test]
    fn resolve_tripwires_replaces_placeholder() {
        let mut tripwires = default_tripwires();

        resolve_tripwires(&mut tripwires, "/repo/project");

        let outside_project = tripwires
            .into_iter()
            .find(|tripwire| tripwire.id == "outside_project")
            .expect("outside_project tripwire");
        assert_eq!(
            outside_project,
            TripwireConfig {
                id: "outside_project".into(),
                kind: TripwireKind::Path {
                    pattern: "!/repo/project/**".into(),
                },
                description: "Writes outside project directory".into(),
                enabled: true,
            }
        );
    }

    #[test]
    fn tripwire_config_serde_round_trip() {
        let original = TripwireConfig {
            id: "git_push".into(),
            kind: TripwireKind::Action {
                category: "git".into(),
                pattern: Some("push".into()),
            },
            description: "Git push to remote".into(),
            enabled: true,
        };

        let encoded = serde_json::to_string(&original).expect("serialize tripwire config");
        let decoded: TripwireConfig =
            serde_json::from_str(&encoded).expect("deserialize tripwire config");

        assert_eq!(decoded, original);
    }
}
