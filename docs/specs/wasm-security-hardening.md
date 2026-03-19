# WASM Security Hardening Specification

**Status:** DRAFT  
**Phase:** Phase 5 (before marketplace goes public)  
**Priority:** Medium  
**Parent doc:** `docs/architecture/open-core-security-model.md`

---

## 1. Goal

Lock down the WASM skill execution environment so that skills cannot exfiltrate data, probe the host environment, or corrupt kernel memory — even if a skill is adversarially crafted.

---

## 2. Threat Model

A malicious or compromised WASM skill could:

1. **Exfiltrate data** — make network requests to external servers with user content
2. **Probe host environment** — enumerate filesystem, read environment variables, discover network topology
3. **Corrupt kernel memory** — exploit bugs in the WASM host boundary (`unsafe` code)
4. **Persist malicious state** — write to unexpected locations that survive skill uninstall
5. **Denial of service** — infinite loops, memory exhaustion, file descriptor exhaustion

---

## 3. WASI Capability Model

### Principle: explicit grant, default deny

Skills receive zero capabilities by default. Each capability must be explicitly declared in the skill manifest and granted at install time.

### Capability inventory

| Capability | Default | Granted when | Risk |
|-----------|---------|-------------|------|
| `fs:read` (skill dir only) | ✅ Granted | Always — skills need to read their own assets | Low |
| `fs:write` (skill dir only) | ✅ Granted | Always — skills may cache state | Low |
| `fs:read` (workspace) | ❌ Denied | Skill declares `workspace_read` in manifest | Medium |
| `fs:write` (workspace) | ❌ Denied | Skill declares `workspace_write` in manifest | High |
| `net:outbound` | ❌ Denied | Skill declares `network` in manifest + domain allowlist | High |
| `env:read` | ❌ Denied | Never for marketplace skills. CLI-only unsigned skills may access | High |
| `clock` | ✅ Granted | Always — needed for timestamps, caching | None |
| `random` | ✅ Granted | Always — needed for UUIDs, sampling | None |
| `stdout/stderr` | ✅ Granted | Always — return values and logging | None |

### Network domain allowlist

Skills that declare `network` must also declare an explicit domain allowlist:

```toml
[capabilities]
network = true
allowed_domains = ["api.weather.gov", "wttr.in"]
```

The WASM host enforces the allowlist at the socket level. Requests to unlisted domains are rejected. Wildcard domains (`*.example.com`) are not supported — each domain must be enumerated.

### Filesystem sandboxing

Even when `workspace_read` or `workspace_write` are granted:
- Kernel source paths are excluded (inherits from kernel blindness deny list)
- Auth/credential paths are excluded
- Config file is read-only at most
- Symlink traversal out of allowed directories is blocked

---

## 4. Resource Limits

### Compiled defaults (not configurable by skills)

| Resource | Limit | Rationale |
|----------|-------|-----------|
| Execution time | 30 seconds per invocation | Prevents infinite loops |
| Memory | 256 MB | Prevents OOM — most skills need <10MB |
| File descriptors | 32 | Prevents FD exhaustion |
| Output size | 1 MB | Prevents return value flooding |
| Stack size | 1 MB | WASM default, prevents stack overflow |

### Timeout enforcement

The WASM runtime uses a fuel-based execution limit (wasmtime `Store::set_fuel`). When fuel is exhausted, execution traps immediately. This is more reliable than wall-clock timeouts for CPU-bound skills.

---

## 5. WASM Host Boundary Security

### The risk

The WASM host interface involves `unsafe` Rust code at the boundary between the WASM runtime and the kernel. A crafted skill could exploit memory safety bugs in this boundary to:
- Read kernel memory
- Corrupt kernel state
- Escape the sandbox

### Mitigations

1. **Minimize the host function surface.** Every host function exposed to WASM is attack surface. Only expose what skills actually need. Audit the current surface and remove anything unused.

2. **Fuzz the host interface.** Integrate fuzzing into CI:
   - `cargo fuzz` targets for every host function
   - Corpus of malformed WASM modules (invalid types, oversized arguments, null pointers)
   - Run fuzzing on every PR that touches the WASM host

3. **Validate all inputs from WASM.** Every value crossing the boundary must be validated:
   - String arguments: check UTF-8 validity, length limits
   - Numeric arguments: range checks
   - Pointer/offset arguments: bounds checks against WASM linear memory

4. **Pin wasmtime version.** Don't auto-update the WASM runtime. Pin to a specific audited version. Update deliberately after reviewing changelogs and security advisories.

---

## 6. Skill Trust Tiers (enforcement)

| Tier | Install method | Capabilities | Network | Persistence |
|------|---------------|-------------|---------|-------------|
| Marketplace-signed | GUI or CLI | Declared in manifest, reviewed at publish | Allowlisted domains only | Skill dir only |
| Locally-signed (by Fawx) | Auto-install or CLI | Declared in manifest | Allowlisted domains only | Skill dir only |
| Unsigned | CLI only | All declared capabilities (no GUI restriction) | Full network (user accepts risk) | Skill dir only |

### GUI install restrictions

The GUI **cannot** install unsigned skills. This is a compiled restriction in the Swift app, not a server-side check. The install endpoint returns an error if the skill signature is invalid or missing, and the GUI does not have a code path to bypass this.

---

## 7. Skill Isolation

### Runtime isolation

Each skill runs in its own WASM instance. Skills cannot:
- Access another skill's memory
- Access another skill's filesystem directory
- Communicate with another skill except through the kernel's tool chain

### Install/uninstall hygiene

- On install: create skill directory, extract assets, verify signature
- On uninstall: delete entire skill directory. No skill-written files survive outside the skill dir.
- Audit: `fawx skill audit` command lists all files written by all skills, flags anything outside expected directories

---

## 8. Testing

### Unit tests
- Capability enforcement: skill without `network` capability cannot make outbound requests
- Domain allowlist: skill with `network` cannot reach unlisted domains
- Filesystem sandbox: skill cannot read kernel paths even with `workspace_read`
- Resource limits: skill exceeding memory/time/FD limits is terminated cleanly
- Input validation: malformed arguments at host boundary are rejected safely

### Fuzz tests
- Every host function has a fuzz target
- Malformed WASM modules are handled gracefully (trap, not crash)
- Oversized and malicious arguments at the boundary don't cause UB

### Integration tests
- End-to-end: install unsigned skill via CLI → works. Attempt via GUI → blocked.
- End-to-end: skill with network capability reaches allowed domain → works. Unlisted domain → blocked.
- End-to-end: skill uninstall leaves no files outside skill directory.

---

## 9. Acceptance Criteria

1. Default-deny capability model enforced for all skills
2. Network domain allowlist enforced at socket level
3. Kernel source paths excluded from all skill filesystem access
4. Resource limits (time, memory, FDs, output) enforced via compiled defaults
5. GUI cannot install unsigned skills (compiled restriction)
6. Fuzz targets exist for every WASM host function
7. Skill uninstall cleanly removes all skill-written files
8. Full test coverage including adversarial WASM modules
