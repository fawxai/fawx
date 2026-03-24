# Contributing to Fawx

Thanks for your interest in contributing! Fawx is an agentic engine that runs
locally, calls LLMs, executes tools, and learns from usage. We welcome
contributions across the engine, skills, documentation, and testing.

## Before You Start

### Sign the CLA

All contributors must sign the Fawx Contributor License Agreement before their
first PR can be merged. When you open your first pull request, the CLA Assistant
bot will prompt you to sign electronically.

- [Individual CLA](docs/legal/CLA-individual.md)
- [Corporate CLA](docs/legal/CLA-corporate.md) (if contributing on behalf of an employer)

### Read the Standards

Fawx has strict engineering standards. Read these before writing code:

- [ENGINEERING.md](ENGINEERING.md) — code quality rules, testing requirements,
  review criteria. Non-negotiable.
- [TASTE.md](TASTE.md) — style preferences and design conventions.

## Getting Started

### Prerequisites

- Rust stable (latest): `rustup update stable`
- Git
- macOS or Linux (Windows via WSL2 works but is not tested in CI)
- For Swift app work: Xcode 16+, macOS 15+

### Build

```bash
git clone https://github.com/fawxai/fawx.git
cd fawx
cargo build --workspace
```

### Test

```bash
cargo test --workspace
```

### Lint

```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
```

All three must pass before submitting a PR. CI enforces this.

## What to Work On

### Good first issues

Look for issues labeled [`good-first-issue`](https://github.com/fawxai/fawx/labels/good-first-issue).
These are scoped, well-defined tasks suitable for new contributors.

### Skills

Building WASM skills is the fastest way to contribute meaningful functionality.
See the [Skill SDK documentation](docs/skills/) and existing skills in the
[fawxai org](https://github.com/fawxai) for reference implementations.

### Bug fixes

Bug reports with reproduction steps are welcome as issues. If you want to fix
one, comment on the issue to avoid duplicate work.

### Features

For larger features, open an issue first to discuss the design. We don't want
you to invest time in a direction we can't merge.

## Pull Request Process

### Branch naming

- `feat/<description>` for features
- `fix/<description>` for bug fixes
- `docs/<description>` for documentation

### PR targets

All PRs target the `dev` branch. We promote `dev` to `staging` to `main` for
releases.

### What a good PR looks like

1. **Focused.** One logical change per PR. Don't bundle unrelated fixes.
2. **Tested.** New behavior has tests. Bug fixes have regression tests.
3. **Documented.** If the change affects user-facing behavior, update docs.
4. **Clean.** `cargo fmt`, `cargo clippy`, `cargo test` all pass.
5. **Described.** PR description explains what changed and why.

### Review criteria

Every PR is reviewed against the standards in ENGINEERING.md:

- Clarity: can someone new understand this in one read?
- Necessity: does every changed line serve the PR's purpose?
- Simplicity: is there a simpler way?
- Completeness: edge cases handled? Tests covering new behavior?
- No regression: does it make existing code harder to maintain?

### After review

Address all review comments. We don't defer findings; everything gets fixed
before merge. If you disagree with a finding, discuss it in the PR thread.

## Code Style Quick Reference

- Functions ≤ 40 lines
- ≤ 5 parameters (use a struct for more)
- No `.unwrap()` outside tests
- No dead code, no TODO without a linked issue
- Names describe behavior, not implementation
- `clippy` clean with `-D warnings`
- `pub` only what needs to be public

## Building Skills

Fawx skills are WASM modules that extend the engine's capabilities. To create
a new skill:

1. Use the skill template: `cargo generate fawxai/skill-template`
2. Implement the `Skill` trait
3. Test locally with `fawx skill install --path ./target/wasm32-wasi/release/`
4. Publish to the marketplace (coming soon)

See [docs/skills/](docs/skills/) for the full SDK reference.

## License

By contributing to Fawx, you agree that your contributions will be licensed
under the [Business Source License 1.1](LICENSE). On the Change Date
(2030-03-23), contributions will become available under the Apache License 2.0.

## Questions?

- Open a [discussion](https://github.com/fawxai/fawx/discussions)
- Join the community on [Discord](https://discord.gg/fawx)

Thanks for helping build Fawx.
