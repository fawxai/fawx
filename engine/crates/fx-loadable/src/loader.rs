//! Skill loader — discovers skill manifests from a directory.
//!
//! V1 implementation: reads `skill.toml` files from immediate subdirectories
//! of a skills directory. No dynamic loading yet — skills are compiled in.
//! The loader provides the discovery framework for future plugin loading.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Loader that discovers skill manifests from a directory tree.
#[derive(Debug)]
pub struct SkillLoader {
    skills_dir: PathBuf,
}

/// Manifest describing a skill, parsed from `skill.toml`.
#[must_use]
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SkillManifest {
    /// Unique skill name.
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Tool names this skill provides.
    #[serde(default)]
    pub tools: Vec<String>,
}

impl SkillLoader {
    /// Create a loader rooted at the given directory.
    #[must_use]
    pub fn new(skills_dir: PathBuf) -> Self {
        Self { skills_dir }
    }

    /// Discover all valid `skill.toml` manifests in subdirectories.
    ///
    /// Invalid or unreadable manifests are logged and skipped.
    /// Results are sorted by skill name for deterministic ordering.
    pub fn discover(&self) -> Vec<SkillManifest> {
        let entries = match std::fs::read_dir(&self.skills_dir) {
            Ok(entries) => entries,
            Err(err) => {
                warn!(
                    path = %self.skills_dir.display(),
                    %err,
                    "failed to read skills directory"
                );
                return Vec::new();
            }
        };

        let mut manifests: Vec<SkillManifest> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| self.load_manifest(&entry.path()))
            .collect();

        // Sort by name for deterministic ordering across platforms
        manifests.sort_by(|a, b| a.name.cmp(&b.name));
        manifests
    }

    /// Try to load a single `skill.toml` from a directory.
    ///
    /// Validates that required fields (`name`, `version`) are non-empty.
    fn load_manifest(&self, dir: &Path) -> Option<SkillManifest> {
        let manifest_path = dir.join("skill.toml");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(err) => {
                warn!(
                    path = %manifest_path.display(),
                    %err,
                    "skipping skill directory: no readable skill.toml"
                );
                return None;
            }
        };

        match toml::from_str::<SkillManifest>(&content) {
            Ok(manifest) => {
                if manifest.name.is_empty() {
                    warn!(
                        path = %manifest_path.display(),
                        "skipping skill manifest: 'name' field is empty"
                    );
                    return None;
                }
                if manifest.version.is_empty() {
                    warn!(
                        path = %manifest_path.display(),
                        "skipping skill manifest: 'version' field is empty"
                    );
                    return None;
                }
                Some(manifest)
            }
            Err(err) => {
                warn!(
                    path = %manifest_path.display(),
                    %err,
                    "skipping invalid skill manifest"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discover_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let loader = SkillLoader::new(tmp.path().to_path_buf());
        assert!(loader.discover().is_empty());
    }

    #[test]
    fn discover_finds_manifests() {
        let tmp = TempDir::new().unwrap();

        let skill_dir = tmp.path().join("greeting");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("skill.toml"),
            r#"
name = "greeting"
version = "0.1.0"
description = "A greeting skill"
tools = ["say_hello", "say_goodbye"]
"#,
        )
        .unwrap();

        let loader = SkillLoader::new(tmp.path().to_path_buf());
        let manifests = loader.discover();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].name, "greeting");
        assert_eq!(manifests[0].version, "0.1.0");
        assert_eq!(manifests[0].tools, vec!["say_hello", "say_goodbye"]);
    }

    #[test]
    fn invalid_manifest_skipped() {
        let tmp = TempDir::new().unwrap();

        // Valid manifest
        let good = tmp.path().join("good");
        std::fs::create_dir(&good).unwrap();
        std::fs::write(
            good.join("skill.toml"),
            r#"
name = "good"
version = "1.0.0"
description = "A good skill"
"#,
        )
        .unwrap();

        // Corrupt manifest (missing required fields)
        let bad = tmp.path().join("bad");
        std::fs::create_dir(&bad).unwrap();
        std::fs::write(bad.join("skill.toml"), "this is not valid toml {{").unwrap();

        let loader = SkillLoader::new(tmp.path().to_path_buf());
        let manifests = loader.discover();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].name, "good");
    }

    #[test]
    fn discover_nonexistent_dir() {
        let loader = SkillLoader::new(PathBuf::from("/nonexistent/path"));
        assert!(loader.discover().is_empty());
    }

    #[test]
    fn discover_skips_manifest_with_empty_name() {
        let tmp = TempDir::new().unwrap();

        let skill_dir = tmp.path().join("noname");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("skill.toml"),
            r#"
name = ""
version = "1.0.0"
description = "missing name"
"#,
        )
        .unwrap();

        let loader = SkillLoader::new(tmp.path().to_path_buf());
        assert!(loader.discover().is_empty());
    }

    #[test]
    fn discover_skips_manifest_with_empty_version() {
        let tmp = TempDir::new().unwrap();

        let skill_dir = tmp.path().join("noversion");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("skill.toml"),
            r#"
name = "noversion"
version = ""
description = "missing version"
"#,
        )
        .unwrap();

        let loader = SkillLoader::new(tmp.path().to_path_buf());
        assert!(loader.discover().is_empty());
    }

    #[test]
    fn discover_skips_empty_skill_toml() {
        let tmp = TempDir::new().unwrap();

        let skill_dir = tmp.path().join("empty");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("skill.toml"), "").unwrap();

        let loader = SkillLoader::new(tmp.path().to_path_buf());
        assert!(loader.discover().is_empty());
    }

    #[test]
    fn discover_ignores_extra_unknown_fields() {
        let tmp = TempDir::new().unwrap();

        let skill_dir = tmp.path().join("extra");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("skill.toml"),
            r#"
name = "extra_skill"
version = "1.0.0"
description = "has extra fields"
author = "someone"
license = "MIT"
extra_unknown_field = true
"#,
        )
        .unwrap();

        let loader = SkillLoader::new(tmp.path().to_path_buf());
        let manifests = loader.discover();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].name, "extra_skill");
    }

    #[test]
    fn discover_returns_manifests_sorted_by_name() {
        let tmp = TempDir::new().unwrap();

        for name in &["zulu", "alpha", "mike"] {
            let skill_dir = tmp.path().join(name);
            std::fs::create_dir(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("skill.toml"),
                format!(
                    "name = \"{}\"\nversion = \"1.0.0\"\ndescription = \"{} skill\"\n",
                    name, name
                ),
            )
            .unwrap();
        }

        let loader = SkillLoader::new(tmp.path().to_path_buf());
        let manifests = loader.discover();
        assert_eq!(manifests.len(), 3);
        assert_eq!(manifests[0].name, "alpha");
        assert_eq!(manifests[1].name, "mike");
        assert_eq!(manifests[2].name, "zulu");
    }
}
