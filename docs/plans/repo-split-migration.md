# Repo Split Migration Plan

**Goal:** Split `abbudjoe/fawx` (private monorepo) into two repos:
- `fawxai/fawx` — public, engine + TUI + skills + docs
- `fawxai/fawx-app` — private, Swift macOS/iOS GUI

---

## Phase 1: Prep (in current monorepo)

### 1.1 Secret Audit
```bash
# Scan full git history for leaked secrets
git log --all -p | grep -iE 'sk-ant|sk-|api[_.]key|bearer|token|password|secret|REDACTED' | head -50

# Check for hardcoded IPs/hostnames
grep -rn '100\.123\.\|100\.93\.\|100\.89\.\|tail9696fb\|8228946254' \
  engine/ tui/ skills/ scripts/ docs/ --include='*.rs' --include='*.toml' --include='*.md'
```

### 1.2 File Classification

**Goes to `fawxai/fawx` (public):**
```
Cargo.toml              (workspace root)
Cargo.lock
engine/crates/*         (all fx-* crates, fawx-ripcord, fawx-test, llama-cpp-sys)
tui/                    (TUI binary)
skills/                 (WASM skill sources)
scripts/                (build, test, release — sanitize cleanup-mac.sh)
docs/                   (sanitized — see 1.3)
assets/
bindings/jni/           (if keeping Android path open)
.github/workflows/      (ci.yml, determinism-eval.yml — drop claude.yml)
install.sh
justfile
ENGINEERING.md
TASTE.md
CONTRIBUTING.md
ARCHITECTURE.md
README.md               (rewrite for public)
```

**Goes to `fawxai/fawx-app` (private):**
```
app/                    (Xcode project, all Swift code, assets)
bindings/swift/         (if any shared Swift bindings)
```

**Goes to neither (strip entirely):**
```
MEMORY.md, memory/
USER.md, SOUL.md, IDENTITY.md
AGENTS.md, BOOTSTRAP.md, SECURITY.md
WORKFLOW_AUTO.md, HEARTBEAT.md
DOCTRINE.md, PROPOSAL_GATE_SPEC.md
parameter-golf/
tmp/
target/
docs/roadmap.html       (internal strategy)
docs/orchestrator-prompt-template.md (internal)
docs/squad-constitution.md (internal)
.github/workflows/claude.yml (Codex integration, internal)
.github/workflows/android-atomic-nightly.yml (no Android yet)
```

### 1.3 Docs Sanitization

**Publish as-is:**
- `docs/architecture/` (AX security model is a selling point)
- `docs/legal/` (privacy, ToS, EULA)
- `docs/SPEC.md`, `docs/WASM_SKILLS.md`
- `docs/testing-patterns.md`

**Review and sanitize:**
- `docs/specs/` — strip internal strategy, competitive positioning, Joe references
- `docs/design/` — review for internal context
- `docs/plans/` — probably strip all (internal roadmaps)
- `docs/decisions/` — review each, publish technical ones
- `docs/runbooks/` — strip deployment-specific details

**Strip:**
- `docs/roadmap.html`, `docs/roadmap/`
- `docs/archive/` (internal history)
- `docs/backlog/` (internal work items)
- `docs/debug/` (internal debugging notes)
- `docs/test-results/` (internal test logs)
- `docs/checklists/` (internal process)
- `docs/deprecated/`
- `docs/parity-map.html`, `docs/phase0-pressure-test.html`, `docs/prototype-bubble.html`

---

## Phase 2: Create Public Repo with Clean History

### Option A: Fresh start (recommended)
Clean initial commit. No history leak risk. Simple.

```bash
# 1. Create temp working directory
mkdir /tmp/fawx-public && cd /tmp/fawx-public
git init

# 2. Copy public files from monorepo
cp -r ~/fawx/engine .
cp -r ~/fawx/tui .
cp -r ~/fawx/skills .
cp -r ~/fawx/scripts .
cp -r ~/fawx/assets .
cp -r ~/fawx/bindings/jni bindings/jni  # if keeping
cp ~/fawx/Cargo.toml ~/fawx/Cargo.lock .
cp ~/fawx/justfile ~/fawx/install.sh .
cp ~/fawx/ENGINEERING.md ~/fawx/TASTE.md ~/fawx/CONTRIBUTING.md .
cp ~/fawx/ARCHITECTURE.md .

# 3. Copy sanitized docs
cp -r ~/fawx/docs .
# Then remove private docs (see 1.3)
rm -rf docs/roadmap* docs/archive docs/backlog docs/debug
rm -rf docs/test-results docs/checklists docs/deprecated
rm -rf docs/plans docs/orchestrator-prompt-template.md
rm -rf docs/squad-constitution.md docs/parity-map.html
rm -rf docs/phase0-pressure-test.html docs/prototype-bubble.html

# 4. Copy sanitized CI
mkdir -p .github/workflows
cp ~/fawx/.github/workflows/ci.yml .github/workflows/
cp ~/fawx/.github/workflows/determinism-eval.yml .github/workflows/
cp ~/fawx/.github/workflows/live-contract-tests.yml .github/workflows/

# 5. Add new files
# LICENSE (Apache 2.0)
# README.md (rewritten for public)
# CODE_OF_CONDUCT.md
# .github/ISSUE_TEMPLATE/
# .github/PULL_REQUEST_TEMPLATE.md

# 6. Fix Cargo workspace paths
# engine/crates/* paths become crates/*
# OR keep engine/ prefix — less churn

# 7. Verify build
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace

# 8. Commit and push
git add -A
git commit -m "Initial public release"
git remote add origin git@github.com:fawxai/fawx.git
git push -u origin main
```

### Option B: git filter-repo (preserves history)
More complex, higher risk of secret leakage, but preserves git blame.

Not recommended for v1. You can always graft history later.

---

## Phase 3: Create Private App Repo

```bash
# 1. Create fawxai/fawx-app (private)
gh repo create fawxai/fawx-app --private

# 2. Copy app directory
mkdir /tmp/fawx-app && cd /tmp/fawx-app
git init
cp -r ~/fawx/app .
cp -r ~/fawx/bindings/swift bindings/swift  # if any

# 3. Add README
# Explain: "Private GUI for Fawx. Engine: https://github.com/fawxai/fawx"

# 4. App needs to know engine API contract
# Option A: pin an engine version / API spec doc
# Option B: git submodule to public repo (for shared types if needed later)
# For now: no dependency — app talks HTTP to engine

# 5. Commit and push
git add -A
git commit -m "Initial app repo"
git remote add origin git@github.com:fawxai/fawx-app.git
git push -u origin main
```

---

## Phase 4: Restructure Cargo Workspace (public repo)

Current structure has `engine/crates/fx-*`. Two options:

### Option A: Keep `engine/` prefix (less churn)
- Workspace members stay `engine/crates/fx-*`
- `tui/` stays at root
- Minimal Cargo.toml changes
- **Recommended** — avoids a massive path rewrite across all crates

### Option B: Flatten to `crates/fx-*`
- Cleaner for a public repo
- Every `path = "../fx-*"` dependency changes
- Every CI path changes
- Not worth it right now

---

## Phase 5: CI & Branch Protection

### Public repo (`fawxai/fawx`)
```yaml
# Branch protection on main:
- Require PR reviews (1)
- Require CI passing
- No force push
- No deletion

# Branch model:
# main (stable releases)
# dev (integration, PRs target here)
# feature/* (contributors)
```

### Private repo (`fawxai/fawx-app`)
```yaml
# Same branch protection
# CI: xcodebuild for macOS + iOS schemes
# No public CI logs (private repo)
```

---

## Phase 6: Update References

- [ ] `fawx.ai` — GitHub link → `fawxai/fawx`
- [ ] `install.sh` — update repo URL
- [ ] `README.md` on public repo — install, architecture, getting started
- [ ] Sparkle appcast — no change (points to fawx.ai, not GitHub)
- [ ] `fawxai` org profile — pin `fawx` repo
- [ ] Archive or make `abbudjoe/fawx` private (keep as backup)
- [ ] Archive `abbudjoe/fawxtui` — superseded by public `fawxai/fawx`
- [ ] Update Clawdio workspace to clone from `fawxai/fawx`

---

## Phase 7: Launch Announcement

- [ ] HN: "Show HN: Fawx — an open-source agentic engine with AX-first security"
- [ ] Reddit: r/rust, r/LocalLLaMA, r/MachineLearning
- [ ] X/Twitter post
- [ ] Discord
- [ ] Email Bart with repo link
- [ ] Reply to Karpathy (if applicable)

---

## Checklist Summary

```
[ ] Secret audit passes
[ ] File classification reviewed
[ ] Docs sanitized
[ ] Public repo created (fawxai/fawx)
[ ] Private app repo created (fawxai/fawx-app)
[ ] cargo clippy + cargo test pass in public repo
[ ] xcodebuild passes in private app repo
[ ] LICENSE added (Apache 2.0)
[ ] README rewritten
[ ] CI workflows configured
[ ] Branch protection set
[ ] fawx.ai links updated
[ ] install.sh updated
[ ] Original monorepo archived
[ ] Announcement drafted
```

---

## Risk Mitigation

1. **Secret leakage**: Fresh commit (Option A) eliminates this entirely. No history = no risk.
2. **Build breakage**: Verify `cargo test --workspace` before pushing public repo.
3. **Missing files**: Diff the file list against working monorepo before publishing.
4. **Contributor confusion**: Clear README with "engine only" scope. Link to fawx.ai for the app.
5. **Premature exposure**: Repo can be created private first, verified, then flipped to public when ready.

---

*Estimated effort: 2-3 hours for the split itself, plus docs/README writing time.*
