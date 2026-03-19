# Spec: AX Phase 3 — OS-Level Enforcement

**Status:** Design spec  
**Depends on:** Phase 1 (Capability Gate — #1471, merged)  
**Can parallel with:** Phase 2 (Tripwire/Ripcord)  
**Architecture ref:** `docs/architecture/ax-kernel-security-model.md`

---

## Overview

OS-level enforcement makes capability boundaries physically impossible to bypass. The app layer defines semantic categories; the OS layer makes them real. An agent with `shell.build` but no `network.external` cannot run `curl` — the syscall fails, regardless of how the agent invokes it.

This is defense in depth. The app-level CapabilityGateExecutor provides friendly structured errors. The OS-level enforcement is the backstop that catches anything the app layer misses.

---

## 1. Enforcement Mechanisms

### Landlock LSM (Filesystem)
Linux 5.13+. Restricts filesystem access to specific paths.

**How we use it:**
- At session start, map capability config to Landlock ruleset
- `capabilities: [filesystem.~/project]` → Landlock allows read/write under `~/project`, read-only for system paths, deny everything else
- Applied to the Fawx process (self-sandboxing) for the session's lifetime

```rust
use landlock::{
    Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, ABI,
};

pub fn apply_filesystem_sandbox(allowed_paths: &[SandboxPath]) -> Result<(), SandboxError> {
    let abi = ABI::V3;  // Landlock v3 (Linux 6.2+, truncation support)
    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?;
    
    for path in allowed_paths {
        let access = match path.mode {
            PathMode::ReadWrite => AccessFs::from_all(abi),
            PathMode::ReadOnly => AccessFs::from_read(abi),
        };
        ruleset = ruleset.add_rule(PathBeneath::new(
            PathFd::new(&path.path)?,
            access,
        ))?;
    }
    
    ruleset.restrict_self()?;
    Ok(())
}
```

**Capability → Landlock mapping:**
| Capability | Landlock rule |
|-----------|--------------|
| `filesystem` (full) | Read/write on working_dir, read on system paths |
| `filesystem.read` | Read-only on working_dir |
| `filesystem.~/project` | Read/write on ~/project, read on system paths |
| No filesystem capability | Read-only on /usr, /lib, /etc (runtime needs) |

### seccomp-bpf (Syscall Filtering)
Restricts which system calls the process can make.

**How we use it:**
- Block dangerous syscalls: `ptrace`, `process_vm_writev`, `mount`, `pivot_root`, `kexec_load`
- Block privilege escalation: `setuid`, `setgid`, `setgroups` (unless root capability granted)
- Always blocked regardless of capability config (hardened baseline)

```rust
use seccompiler::{
    BpfProgram, SeccompAction, SeccompFilter, SeccompRule,
};

pub fn apply_syscall_sandbox() -> Result<(), SandboxError> {
    let filter = SeccompFilter::new(
        // Default action: allow (we're a denylist, not allowlist)
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into()?,
    )?;
    
    // Deny dangerous syscalls
    let denied = [
        libc::SYS_ptrace,
        libc::SYS_process_vm_writev,
        libc::SYS_process_vm_readv,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_kexec_load,
        libc::SYS_kexec_file_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_setuid,
        libc::SYS_setgid,
        libc::SYS_setgroups,
    ];
    
    // Apply filter
    let program: BpfProgram = filter.try_into()?;
    seccompiler::apply_filter(&program)?;
    Ok(())
}
```

### Network Namespaces (Egress Control)
Restricts network access.

**How we use it:**
- `network.localhost` → Create network namespace with loopback only
- `network.external` → No namespace (full access)
- No network capability → Namespace with loopback only (agent can talk to local server)

**Implementation note:** Network namespaces require `CAP_NET_ADMIN` to create, which means either:
1. Fawx runs with ambient capability (not ideal)
2. A privileged helper creates the namespace before Fawx starts
3. Use iptables/nftables instead of namespaces (less isolation but no privilege needed)

**Recommendation:** Option 3 for Phase 3 MVP. Use nftables rules scoped to the Fawx process UID. Evaluate namespaces for FawxShell.

```rust
pub fn apply_network_sandbox(config: &NetworkCapability) -> Result<(), SandboxError> {
    match config {
        NetworkCapability::Full => Ok(()),  // No restriction
        NetworkCapability::LocalhostOnly => {
            // Add nftables rule: block outbound except 127.0.0.0/8 and ::1
            apply_nftables_egress_filter()?;
            Ok(())
        }
        NetworkCapability::None => {
            // Block all outbound including localhost
            // (How does the agent talk to the Fawx HTTP server? Via stdin/stdout, not network)
            apply_nftables_block_all()?;
            Ok(())
        }
        NetworkCapability::AllowList(hosts) => {
            // Allow specific hosts only
            apply_nftables_allowlist(hosts)?;
            Ok(())
        }
    }
}
```

---

## 2. Sandbox Lifecycle

### Session start
```
1. Parse capability config for session
2. Map capabilities → OS rules (Landlock paths, seccomp syscalls, network policy)
3. Apply seccomp filter (always, hardened baseline)
4. Apply Landlock ruleset (based on filesystem capabilities)
5. Apply network policy (based on network capabilities)
6. Session runs with OS-enforced boundaries
```

### Important: Irreversible
Landlock and seccomp are **irreversible** once applied to a process. You cannot expand permissions after restriction. This means:

- Capability grants mid-session require **session restart** (new process with updated rules)
- The capability negotiation from Phase 1 ("I need network access") must restart the session
- This is actually a security feature — no way for the agent to expand its own sandbox

### Self-sandboxing vs external sandboxing
Phase 3 uses **self-sandboxing**: the Fawx process restricts itself. This is simpler but means:
- The sandbox is only as strong as the Fawx code that applies it
- A bug before sandbox application = no sandbox
- The process could theoretically skip sandboxing

FawxShell (future) would use **external sandboxing**: a parent process creates the sandbox and launches Fawx inside it. Stronger guarantees but more complexity.

---

## 3. Capability Config Extension

### New config section
```toml
[security]
# OS-level enforcement (Phase 3)
os_sandbox = true  # Enable Landlock + seccomp + network policy

[security.sandbox]
# Filesystem paths the agent can access
filesystem_allow = [
    { path = "~/project", mode = "rw" },
    { path = "~/.fawx", mode = "rw" },
    { path = "/tmp", mode = "rw" },
]

# Network policy
network = "localhost"  # "full", "localhost", "none", or list of allowed hosts

# Additional blocked syscalls (beyond hardened baseline)
blocked_syscalls = []
```

### Mapping from existing presets
| Preset | Filesystem | Network | Syscalls |
|--------|-----------|---------|----------|
| Open | working_dir + /tmp (rw), system (ro) | full | baseline deny |
| Standard | working_dir + /tmp (rw), system (ro) | localhost | baseline deny |
| Restricted | working_dir (ro), /tmp (rw) | none | baseline deny + extra |

---

## 4. Error Handling

When OS enforcement blocks an action, the agent sees a system error:
- Landlock: `EACCES` on file operations → tool returns "Permission denied: /path/to/file"
- seccomp: `EPERM` on syscall → process may crash (seccomp violations are harsh)
- nftables: `ENETUNREACH` or `ECONNREFUSED` → tool returns "Network unreachable"

The app-level CapabilityGateExecutor should catch these BEFORE they hit the OS level. The OS level is the backstop for:
1. Bugs in the app-level check
2. Actions routed through shell commands that bypass tool-level checks
3. Malicious tool implementations

---

## 5. Platform Support

| Platform | Landlock | seccomp | Network NS | nftables |
|----------|---------|---------|-----------|----------|
| Linux 6.2+ | ✅ Full | ✅ Full | ✅ (needs CAP) | ✅ |
| Linux 5.13-6.1 | ⚠️ v1/v2 | ✅ Full | ✅ (needs CAP) | ✅ |
| Linux < 5.13 | ❌ | ✅ Full | ✅ (needs CAP) | ✅ |
| macOS | ❌ | ❌ | ❌ | ❌ |
| Windows | ❌ | ❌ | ❌ | ❌ |

**macOS alternative:** App Sandbox (entitlements) or `sandbox-exec` (deprecated but functional). For macOS, Phase 3 is deferred to FawxShell which would use a different enforcement mechanism.

**Graceful degradation:** If Landlock/seccomp are unavailable (older kernel, non-Linux), log a warning and rely on app-level enforcement only. The capability gate still works — OS enforcement is the bonus layer.

```rust
pub fn apply_sandbox(config: &SandboxConfig) -> Result<SandboxStatus, SandboxError> {
    let mut status = SandboxStatus::default();
    
    // Always attempt seccomp (broad Linux support)
    match apply_syscall_sandbox() {
        Ok(()) => status.seccomp = true,
        Err(e) => {
            tracing::warn!("seccomp not available: {e}");
            status.seccomp = false;
        }
    }
    
    // Landlock requires 5.13+
    match apply_filesystem_sandbox(&config.filesystem_allow) {
        Ok(()) => status.landlock = true,
        Err(e) => {
            tracing::warn!("Landlock not available: {e}");
            status.landlock = false;
        }
    }
    
    // Network policy via nftables
    match apply_network_sandbox(&config.network) {
        Ok(()) => status.network = true,
        Err(e) => {
            tracing::warn!("Network sandbox not available: {e}");
            status.network = false;
        }
    }
    
    Ok(status)
}
```

---

## 6. Crate Structure

### New crate: fx-sandbox
```
engine/crates/fx-sandbox/
  src/
    lib.rs              # Public API: apply_sandbox(), SandboxConfig, SandboxStatus
    landlock.rs         # Filesystem enforcement
    seccomp.rs          # Syscall filtering
    network.rs          # Network policy (nftables)
    config.rs           # SandboxConfig, capability → rule mapping
  Cargo.toml            # deps: landlock, seccompiler, nix
```

### Integration point
In `fx-cli/startup.rs`, after loading config and before building the executor chain:
```rust
// Apply OS-level sandbox based on session capabilities
if config.security.os_sandbox {
    let sandbox_config = SandboxConfig::from_capabilities(&config.permissions);
    let status = fx_sandbox::apply_sandbox(&sandbox_config)?;
    tracing::info!("OS sandbox: seccomp={}, landlock={}, network={}", 
        status.seccomp, status.landlock, status.network);
}
```

---

## 7. Test Plan

### Unit tests (fx-sandbox)
- Config mapping: capability presets → sandbox rules
- Landlock ruleset construction (mock, don't actually apply in tests)
- seccomp filter construction
- Network policy rule generation
- Graceful degradation when features unavailable

### Integration tests (Linux only, CI)
- Apply Landlock → verify file access denied outside allowed paths
- Apply seccomp → verify blocked syscalls return EPERM
- Apply network policy → verify outbound connections blocked
- Full sandbox → verify agent tools respect boundaries
- Capability grant → verify session restart required

### Smoke test
- Start Fawx with Standard preset + os_sandbox=true
- Verify: file write in project dir works
- Verify: file write outside project dir fails
- Verify: `curl` from shell tool fails (network blocked)
- Verify: agent receives structured error from app layer (not raw EACCES)

---

## 8. Dependencies

### Rust crates
- `landlock` (Rust bindings for Landlock LSM) — well-maintained, used by systemd
- `seccompiler` (Amazon's seccomp-bpf library) — battle-tested in Firecracker
- `nix` (Unix/Linux syscall bindings) — already in workspace

### System requirements
- Linux 5.13+ for Landlock (6.2+ for full v3 support)
- Kernel compiled with `CONFIG_SECURITY_LANDLOCK=y` (default on most distros)
- nftables userspace tools for network policy
- No root required for Landlock/seccomp (self-sandboxing)
- Root or CAP_NET_ADMIN required for nftables rules

---

## 9. Phase 3 vs FawxShell

Phase 3 is **self-sandboxing**: Fawx restricts itself. Good enough for:
- Development workstations (primary use case)
- Single-user deployments
- Defense in depth alongside app-level enforcement

FawxShell (future) is **external sandboxing**: a parent process creates the sandbox. Better for:
- Multi-tenant deployments
- Enterprise/server use cases
- Stronger guarantees (sandbox can't be skipped)
- macOS/Windows support via platform-native mechanisms

Phase 3 validates the approach and maps capabilities to OS primitives. FawxShell reuses the mapping logic but applies it from outside.
