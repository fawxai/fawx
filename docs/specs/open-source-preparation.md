# Open Source Preparation Specification

**Status:** DRAFT  
**Phase:** Post-Phase 5, pre-launch  
**Priority:** High — required before any public release  
**Parent doc:** `docs/architecture/open-core-security-model.md`

---

## 1. Goal

Extract the open-source components of Fawx (skill SDK, loadable layer interfaces, channel traits, marketplace protocol) into public repositories with clean APIs, documentation, and contribution infrastructure.

---

## 2. What Goes Open Source

### Repositories to create

| Repo | Contents | License |
|------|---------|---------|
| `fawxai/skill-sdk` | Skill authoring SDK, WASM bridge types, manifest schema, example skills | Apache 2.0 |
| `fawxai/marketplace-protocol` | Skill discovery API, signature verification, manifest validation | Apache 2.0 |
| `fawxai/channels` | Channel trait definition, reference implementations (webhook, etc.) | Apache 2.0 |
| `fawxai/fawx-docs` | Public documentation, architecture guides, tutorials | CC BY 4.0 |

### What stays proprietary

- `engine/crates/fx-kernel/` — loop orchestrator, policy engine
- `engine/crates/fx-security/` — proposal gate, enforcement
- `engine/crates/fx-gate/` — gate implementation
- `engine/crates/fx-consensus/` — experiment protocol internals
- `engine/crates/fx-policy/` — policy engine
- `engine/crates/fx-fleet/` — fleet coordination internals
- Swift app source (until/unless decided otherwise)
- Build infrastructure, CI secrets, release pipeline

---

## 3. Extraction Process

### Step 1: Identify boundary

Audit every crate in `engine/crates/` and classify as kernel (proprietary) or loadable (open-source candidate). The boundary must be clean — no open crate should depend on a proprietary crate's internals.

### Step 2: Define public traits

Extract the trait definitions that form the kernel↔loadable interface:
- `ToolExecutor` trait (how skills call tools)
- `SkillManifest` types (how skills declare capabilities)
- `SkillResult` types (how skills return values)
- `ChannelTrait` (how channels send/receive messages)
- `MemoryStore` trait (how skills interact with memory)

These traits are the public API. They must be stable, versioned, and documented.

### Step 3: Clean room extraction

For each open-source repo:
1. Create fresh repo (not fork of the main repo — avoid git history leaking proprietary code)
2. Copy only the relevant source files
3. Ensure no `use fx_kernel::*` or similar imports exist
4. Add LICENSE, README, CONTRIBUTING.md, SECURITY.md
5. Set up independent CI (GitHub Actions)
6. Publish initial version to crates.io (for Rust crates)

### Step 4: Dependency audit

Run a full dependency check:
- No open-source crate transitively depends on a proprietary crate
- No proprietary types appear in public API signatures
- No proprietary error types leak through public error enums
- No hardcoded paths, URLs, or constants that reference proprietary infrastructure

---

## 4. Contribution Infrastructure

### Per-repo requirements

- `CONTRIBUTING.md` — code style, PR process, testing requirements
- `SECURITY.md` — responsible disclosure process
- `CODE_OF_CONDUCT.md` — standard CoC
- Issue templates — bug report, feature request, skill idea
- PR template — checklist for contributor PRs
- CI — lint, test, build on every PR

### Contributor License Agreement (CLA)

Required for all contributions. Options:
- **Apache ICLA** (individual) — standard, well-understood
- **CLA bot** (GitHub App) — automated check on PRs

The CLA ensures we can relicense or incorporate contributions into the proprietary kernel if needed (e.g., a community-contributed trait improvement that should be in the kernel).

### Maintainer team

- Joe: repo owner, merge authority
- Fawx community: issue triage, code review (once established)
- Bot automation: CLA check, CI, stale issue cleanup

---

## 5. Documentation Plan

### Public docs site (`docs.fawx.ai` or similar)

- **Getting started** — install Fawx, create your first skill
- **Skill SDK reference** — API docs, manifest schema, capability model
- **Architecture overview** — kernel/loadable split (public-facing version, no proprietary details)
- **Marketplace guide** — publish a skill, signing, review process
- **Channel development** — implement a custom channel
- **Security model** — public version of the open-core security doc (redact threat model specifics)

### What NOT to publish

- Kernel architecture details beyond the public trait interfaces
- Threat model specifics (attack scenarios, break analysis)
- Internal roadmap items not yet shipped
- Experiment protocol internals

---

## 6. Legal Checklist

- [ ] Choose license for each open-source repo
- [ ] Draft CLA
- [ ] Audit all open-source code for accidental proprietary inclusion
- [ ] Review third-party dependency licenses for compatibility
- [ ] Draft SECURITY.md with responsible disclosure process
- [ ] Review trademark usage (Fawx name, logo) in open-source context
- [ ] Consider trademark policy for community skills using the Fawx name

---

## 7. Acceptance Criteria

1. Skill SDK repo is public with working examples, docs, and CI
2. Marketplace protocol repo is public with spec and reference implementation
3. Channel trait repo is public with webhook reference implementation
4. No open-source repo contains proprietary code or git history
5. CLA bot configured and enforcing on all repos
6. Public docs site live with getting-started, SDK reference, and architecture overview
7. All dependencies audited for license compatibility
8. Clean compilation boundary — open crates build independently of proprietary crates
