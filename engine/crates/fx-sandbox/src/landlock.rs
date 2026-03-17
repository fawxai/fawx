//! Landlock LSM filesystem sandboxing.

use crate::config::{PathMode, SandboxPath};
use landlock::{
    Access, AccessFs, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI,
};

/// Apply Landlock filesystem sandbox.
/// Restricts filesystem access to the specified paths.
/// This is irreversible for the current process.
pub fn apply_filesystem_sandbox(allowed_paths: &[SandboxPath]) -> Result<(), SandboxError> {
    if allowed_paths.is_empty() {
        return Ok(());
    }

    let abi = ABI::V3;
    let mut ruleset = create_ruleset(abi)?;

    for sandbox_path in allowed_paths {
        ruleset = add_rule(ruleset, sandbox_path, abi)?;
    }

    ruleset
        .restrict_self()
        .map_err(|error| SandboxError::Landlock(format!("restrict_self: {error}")))?;

    Ok(())
}

fn create_ruleset(abi: ABI) -> Result<landlock::RulesetCreated, SandboxError> {
    Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .map_err(|error| SandboxError::Landlock(format!("ruleset create: {error}")))?
        .create()
        .map_err(|error| SandboxError::Landlock(format!("ruleset init: {error}")))
}

fn add_rule(
    ruleset: landlock::RulesetCreated,
    sandbox_path: &SandboxPath,
    abi: ABI,
) -> Result<landlock::RulesetCreated, SandboxError> {
    let access = match sandbox_path.mode {
        PathMode::ReadWrite => AccessFs::from_all(abi),
        PathMode::ReadOnly => AccessFs::from_read(abi),
    };

    let Some(rule) = path_rule(sandbox_path, access) else {
        return Ok(ruleset);
    };

    ruleset
        .add_rule(rule)
        .map_err(|error| SandboxError::Landlock(format!("add rule: {error}")))
}

fn path_rule(
    sandbox_path: &SandboxPath,
    access: BitFlags<AccessFs>,
) -> Option<PathBeneath<PathFd>> {
    match PathFd::new(&sandbox_path.path) {
        Ok(fd) => Some(PathBeneath::new(fd, access)),
        Err(error) => {
            tracing::warn!(
                "Landlock: skipping path {} ({})",
                sandbox_path.path.display(),
                error
            );
            None
        }
    }
}

/// Sandbox application errors.
#[derive(Debug)]
pub enum SandboxError {
    Landlock(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Landlock(msg) => write!(f, "Landlock error: {msg}"),
        }
    }
}

impl std::error::Error for SandboxError {}
