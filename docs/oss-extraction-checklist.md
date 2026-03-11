# OSS Extraction Checklist — fawxai/fawx

## Pre-extraction (do first)

- [ ] Create fresh repo `fawxai/fawx` (public, empty)
- [ ] Squash private history into clean initial commit (no secret leaks in git log)
- [ ] Run `git log --all -p | grep -iE 'sk-ant|sk-|api.key|token|password|secret'` on source repo to catch anything
- [ ] Verify no hardcoded IPs, hostnames, or Tailscale addresses in source

## Files to add

- [ ] `LICENSE` (MIT)
- [ ] `README.md` — install, quick start, architecture, link to fawx.ai
- [ ] `CONTRIBUTING.md` — how to contribute, PR process, code standards
- [ ] `CODE_OF_CONDUCT.md`
- [ ] `.github/ISSUE_TEMPLATE/` — bug report, feature request
- [ ] `.github/PULL_REQUEST_TEMPLATE.md`
- [ ] `.github/workflows/ci.yml` — public CI (build + test + clippy)

## Files to strip (do NOT publish)

- [ ] `MEMORY.md`, `memory/` directory
- [ ] `USER.md`, `SOUL.md`, `IDENTITY.md`
- [ ] `AGENTS.md`, `BOOTSTRAP.md`, `SECURITY.md`
- [ ] `WORKFLOW_AUTO.md`, `HEARTBEAT.md`
- [ ] `docs/specs/` — review each spec, strip strategy/competitive notes
- [ ] `docs/roadmap.html` — internal only
- [ ] Any `.env`, `.secret`, credential files
- [ ] `tui/assets/` — verify only `fawx-new.png` (no personal images)

## Files to review and sanitize

- [ ] `engine/crates/fx-config/` — default config should have no real keys/endpoints
- [ ] `engine/crates/fx-cli/src/commands/setup.rs` — wizard should work for new users
- [ ] All `ENGINEERING.md` / `TASTE.md` — fine to publish (shows quality bar)
- [ ] `docs/specs/proof-of-fitness.md` — publish (this IS the claim)
- [ ] Test fixtures — no real API responses with identifying info

## README content

- Project description (what Fawx is, why it exists)
- Architecture diagram (kernel / loadable / shells)
- Quick start (install, setup, chat, TUI)
- Feature list (tools, memory, skills, safety, multi-provider)
- Proof of Fitness section (experimental, link to spec)
- Skill ecosystem (WASM, marketplace)
- Contributing link
- License

## Post-extraction

- [ ] Set up branch protection on `fawxai/fawx` (main only, require CI)
- [ ] Update fawx.ai GitHub links to point to `fawxai/fawx`
- [ ] Update `install.sh` to reference public repo
- [ ] Pin `fawxai/fawx` on GitHub org profile
- [ ] Post announcement (X, HN, Reddit/r/rust, Discord)
- [ ] Reply to Karpathy with repo link + protocol spec
- [ ] Mirror CI status badge on README and fawx.ai

## Tags

- [ ] Tag initial release: `v0.1.0-alpha`
- [ ] Experiment pipeline tagged `experimental` in docs and CLI help
- [ ] `fawx experiment` commands show "experimental" warning on first use

## Timeline

Ship when: core engine works, TUI works, experiment runs (even if scoring needs tuning).
Don't wait for: perfect scores, distributed fleet, local models, self-training.
