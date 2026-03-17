//! Landlock LSM filesystem sandboxing.

use crate::{
    config::{PathMode, SandboxPath},
    SandboxError,
};
use landlock::{
    Access, AccessFs, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI,
};

/// Apply Landlock filesystem sandbox.
/// Restricts filesystem access to the specified paths.
/// This is irreversible for the current process.
pub fn apply_filesystem_sandbox(allowed_paths: &[SandboxPath]) -> Result<(), SandboxError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_ruleset_succeeds_without_any_allowed_paths() {
        let result = create_ruleset(ABI::V3);

        assert!(result.is_ok());
    }
}
