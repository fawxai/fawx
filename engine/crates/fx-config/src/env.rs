//! Path expansion helpers for user-facing config fields.

use crate::FawxConfig;
use std::path::{Path, PathBuf};

/// Expand a leading `~` in a path to the user's home directory.
///
/// Only expands `~` at the very start of the path (i.e., `~/.fawx` becomes
/// `/home/user/.fawx`). Paths like `foo/~/bar` or absolute paths are returned
/// unchanged. Returns the original path if the home directory cannot be
/// determined.
pub(crate) fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    // ~user paths and everything else: return as-is
    path.to_path_buf()
}

/// Apply tilde expansion to an optional path field.
fn expand_tilde_opt(path: &mut Option<PathBuf>) {
    if let Some(p) = path.as_mut() {
        let original = p.clone();
        *p = expand_tilde(&original);
        if *p != original {
            tracing::debug!(
                "config path expanded: {} -> {}",
                original.display(),
                p.display()
            );
        }
    }
}

fn expand_tilde_string_opt(path: &mut Option<String>) {
    if let Some(path_str) = path.as_mut() {
        let original = path_str.clone();
        let expanded = expand_tilde(Path::new(&original));
        let expanded_str = expanded.to_string_lossy().into_owned();
        if expanded_str != original {
            tracing::debug!("config path expanded: {} -> {}", original, expanded_str);
            *path_str = expanded_str;
        }
    }
}

impl FawxConfig {
    /// Expand `~` to the user's home directory in all user-facing path configs.
    pub(crate) fn expand_paths(&mut self) {
        expand_tilde_opt(&mut self.general.data_dir);
        expand_tilde_string_opt(&mut self.logging.log_dir);
        expand_tilde_opt(&mut self.tools.working_dir);
        expand_tilde_opt(&mut self.self_modify.proposals_dir);
    }
}
