//! seccomp-bpf syscall filtering.
//!
//! Blocks privilege escalation by enabling `no_new_privs`.
//! This is a hardened baseline applied regardless of capability config.

use crate::SandboxError;
#[cfg(target_os = "linux")]
use nix::sys::prctl;

/// Apply seccomp syscall filter.
/// Phase 3 MVP enables `no_new_privs` as a non-bypassable baseline.
/// This is irreversible for the current process.
pub fn apply_syscall_sandbox() -> Result<(), SandboxError> {
    prctl::set_no_new_privs()
        .map_err(|error| SandboxError::Seccomp(format!("set_no_new_privs: {error}")))?;

    tracing::info!("seccomp: no_new_privs enabled");
    Ok(())
}
