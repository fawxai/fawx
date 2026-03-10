# Spec: `fawx update` Command

**Issue:** #1302  
**Phase:** 3b (Ops)  
**Status:** Draft  
**Author:** Clawdio  
**Date:** 2026-03-10

---

## Problem

Updating a running Fawx instance requires 5-6 manual steps:

```bash
kill $(lsof -ti :8400)          # or Ctrl+C
git pull origin dev
cargo build --release            # engine
cargo build --release -p fawx-tui  # TUI
cd skills && ./build.sh --install && cd ..
./target/release/fawx serve --http
```

There's no single command that does the full cycle. `fawx restart --rebuild` only rebuilds the engine binary (`fx-cli`), skips the TUI, skips skills, and doesn't pull.

## Solution

Add `fawx update [BRANCH]` — a single command that pulls, builds everything, installs skills, and restarts the running instance.

## CLI Interface

```
fawx update [BRANCH] [OPTIONS]

Arguments:
  [BRANCH]    Git branch to pull from [default: current branch]
              Common values: dev, staging, main

Options:
  --no-pull       Skip git pull (rebuild from current working tree)
  --no-skills     Skip WASM skill rebuild and install
  --no-restart    Build only, don't restart the running instance
  --force         Continue even if working tree has uncommitted changes
```

### Examples

```bash
# Full update from dev branch
fawx update dev

# Update from staging (pre-release)
fawx update staging

# Rebuild everything from current tree without pulling
fawx update --no-pull

# Just pull and build, restart manually later
fawx update dev --no-restart
```

## Execution Flow

```
fawx update dev
│
├── 1. Pre-flight checks
│   ├── Verify repo root (Cargo.toml + engine/crates/fx-cli exists)
│   ├── Check for uncommitted changes (fail unless --force)
│   └── Verify cargo + wasm32 target available
│
├── 2. Git pull (unless --no-pull)
│   ├── git fetch origin
│   ├── git checkout <BRANCH> (if different from current)
│   └── git pull origin <BRANCH> --ff-only
│       └── Fail on merge conflicts (user must resolve manually)
│
├── 3. Build engine
│   └── cargo build --release -p fx-cli
│
├── 4. Build TUI
│   └── cargo build --release -p fawx-tui
│
├── 5. Build + install skills (unless --no-skills)
│   ├── skills/build.sh --install
│   └── Report: "Installed N skills to ~/.fawx/skills/"
│
├── 6. Restart (unless --no-restart)
│   ├── Find running fawx serve (PID file or process search)
│   ├── SIGTERM → wait for exit (10s timeout)
│   ├── Start new instance: ./target/release/fawx serve --http
│   └── Verify: wait for PID file + port 8400 responsive
│
└── 7. Summary
    ├── Git: pulled <branch> (abc1234..def5678)
    ├── Engine: built (release)
    ├── TUI: built (release)
    ├── Skills: 8 installed
    └── Server: restarted (pid 12345, port 8400)
```

## Detailed Behavior

### Pre-flight Checks

1. **Repo root detection**: Reuse existing `resolve_repo_root()` from `restart.rs`. Must find `Cargo.toml` with `[workspace]` and `engine/crates/fx-cli/Cargo.toml`.
2. **Dirty tree check**: Run `git status --porcelain`. If non-empty and `--force` not set, print warning and exit with code 1. Message: `"Working tree has uncommitted changes. Use --force to update anyway."`
3. **Toolchain check**: Verify `cargo` in PATH. If `--no-skills` not set, verify `wasm32-wasip1` target installed (`rustup target list --installed | grep wasm32-wasip1`).

### Git Pull

- `git fetch origin` first (always, to update remote refs).
- If `BRANCH` is provided and differs from current branch: `git checkout <BRANCH>`.
- `git pull origin <BRANCH> --ff-only`. Fast-forward only — if the local branch has diverged, fail with: `"Cannot fast-forward. Resolve manually: git pull --rebase origin <BRANCH>"`.
- Print: `"Updated <branch>: <old-sha>..<new-sha>"` or `"Already up to date on <branch> (<sha>)"`.

### Build Steps

Each build step prints a single status line:
- `"Building engine..."` → `"Engine built (release, 42s)"`
- `"Building TUI..."` → `"TUI built (release, 18s)"`
- `"Building skills..."` → `"8 skills built and installed"`

Build failures abort the update. The running instance is NOT stopped until all builds succeed. This is critical — a failed build should never leave the user with no running server.

### Restart

Reuse the existing `restart.rs` infrastructure:
1. `resolve_target_pid()` — find running instance
2. `send_signal(pid, SIGTERM)` — graceful shutdown
3. `wait_for_exit(pid, 10s)` — poll until gone
4. `spawn_serve(release_binary)` — start new instance
5. Post-start verification: wait up to 5s for PID file to appear and port 8400 to accept connections.

If no running instance found: skip stop, just start. Print: `"No running instance found, starting fresh."`

### Error Handling

| Scenario | Behavior |
|----------|----------|
| Not in a git repo | `"Not a git repository. Run from the fawx source directory."` |
| Dirty working tree | `"Uncommitted changes. Use --force or commit first."` |
| Branch doesn't exist | `"Branch '<name>' not found on remote."` |
| Merge conflict | `"Cannot fast-forward. Resolve: git pull --rebase origin <branch>"` |
| Cargo build fails | Print cargo output, exit. Server still running. |
| Skill build fails | Print error, continue with restart (skills are optional) |
| No running instance | Start fresh instead of restart |
| Stop timeout | `"Timed out waiting for old instance to exit. Kill manually: kill <pid>"` |
| Port still busy | `"Port 8400 still in use after restart. Check for zombie processes."` |

### Exit Codes

- `0` — success
- `1` — pre-flight check failed (dirty tree, missing tools)
- `2` — git operation failed (pull, checkout)
- `3` — build failed
- `4` — restart failed

## Implementation

### New file: `engine/crates/fx-cli/src/commands/update.rs`

```rust
pub(crate) struct UpdateArgs {
    pub(crate) branch: Option<String>,
    pub(crate) no_pull: bool,
    pub(crate) no_skills: bool,
    pub(crate) no_restart: bool,
    pub(crate) force: bool,
}
```

### Integration with existing code

- Reuse `restart.rs`: `resolve_target_pid`, `send_signal`, `wait_for_exit`, `spawn_serve`, `resolve_repo_root`
- Reuse `restart.rs::run_rebuild` pattern but extend to build TUI too
- Git operations: use `std::process::Command` with `git` (same pattern as `run_rebuild` uses `cargo`)
- Skill build: shell out to `skills/build.sh --install`

### CLI registration in `main.rs`

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing ...
    /// Pull latest code, rebuild, and restart
    Update(update::UpdateArgs),
}
```

## Testing

### Unit tests (in `update.rs`)

1. **Pre-flight: dirty tree detected** — mock git status returning non-empty, verify error without `--force`
2. **Pre-flight: dirty tree with --force** — mock git status non-empty, verify continues
3. **Git pull: fast-forward success** — mock git pull, verify sha range printed
4. **Git pull: already up to date** — mock git pull "Already up to date", verify message
5. **Git pull: diverged branch** — mock git pull failure, verify error message
6. **Branch checkout** — verify git checkout called when branch differs from current
7. **Build order: server not stopped before build succeeds** — verify SIGTERM not sent until after build commands succeed
8. **No running instance: starts fresh** — mock no PID file, verify spawn_serve called without stop
9. **Skill build failure: continues to restart** — mock skill build failure, verify restart still happens
10. **--no-pull skips git** — verify no git commands run
11. **--no-skills skips skill build** — verify skills/build.sh not called
12. **--no-restart skips restart** — verify no signal/spawn calls

### Integration test (manual, on Mac Mini)

```bash
# From fawx repo root with server running
fawx update dev
# Verify: pulled, built, skills installed, server restarted, port 8400 responsive
```

## Summary Output Example

```
$ fawx update dev

Pre-flight checks... OK
Fetching origin...
Updated dev: 84d00a59..f1c23b47 (3 commits)
Building engine... done (38s)
Building TUI... done (15s)
Building skills... 8 skills built and installed
Stopping fawx (pid 42301)...
Starting fawx serve --http...
Server ready (pid 42589, port 8400)

✓ Update complete
```

## Not In Scope

- **Auto-update / version checking**: That's #1302's broader scope. This spec covers the manual `fawx update` command only.
- **Remote fleet updates**: Future work. This is local-machine only.
- **Rollback**: If the new build is broken, user runs `git checkout <old-sha>` and `fawx update --no-pull`.
- **Self-replacing binary**: The update command itself might be stale after pull. This is acceptable — the new binary is what gets started by `spawn_serve`. The update command is just the orchestrator.
