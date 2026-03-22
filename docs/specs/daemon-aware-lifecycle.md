# Fix: Daemon-Aware Lifecycle Commands

## Problem

`fawx stop` and `fawx start` are completely unaware of the LaunchAgent
daemon. When a LaunchAgent with `KeepAlive: true` is installed:

- `fawx stop` kills the process, but launchd immediately restarts it
- `fawx start` spawns a new process directly, competing with launchd
- `fawx restart` sends SIGHUP but doesn't interact with launchd at all

### Current behavior
```
fawx stop     → SIGTERM → process dies → launchd restarts it → "stopped" but still running
fawx start    → spawns new process → now TWO fawx processes (launchd + direct)
fawx restart  → SIGHUP → works for config reload, but binary changes need full stop/start
```

## Fix

### fawx stop

1. Check `fx_api::launchagent::status()` — is a LaunchAgent installed and loaded?
2. If loaded: `launchctl bootout gui/<uid> <plist_path>` to stop the service
   AND prevent launchd from restarting it
3. If not loaded: current behavior (SIGTERM → SIGKILL → remove PID file)
4. Print what happened: "Stopped fawx (LaunchAgent unloaded)" vs "Stopped fawx"

### fawx start

1. Check if a LaunchAgent plist exists at the expected path
2. If plist exists: `launchctl bootstrap gui/<uid> <plist_path>` to let launchd
   manage the process
3. If no plist: current behavior (spawn directly)
4. Print: "Started fawx (LaunchAgent)" vs "Started fawx"

### fawx restart

1. If LaunchAgent loaded: bootout + bootstrap (full daemon restart)
2. If not loaded but process running: SIGTERM + spawn (current --hard behavior)
3. Default (no --hard): SIGHUP for config-only reload (unchanged)

### fawx restart --rebuild

1. If LaunchAgent loaded: bootout → cargo build → update plist binary path →
   bootstrap
2. If not loaded: current behavior (stop → build → start)

## Files to Change

1. `engine/crates/fx-cli/src/commands/start_stop.rs`
   - Import `fx_api::launchagent`
   - `execute_stop`: check launchagent status, bootout if loaded
   - `execute_start`: check for plist, bootstrap if exists
   - Add `LaunchAgentControl` to the `ProcessControl` trait or handle
     separately

2. `engine/crates/fx-cli/src/restart.rs`
   - `execute_restart`: check daemon mode for --hard and --rebuild
   - `stop_and_start`: use bootout/bootstrap when daemon is active

3. `engine/crates/fx-api/src/launchagent.rs`
   - Ensure `bootout` and `bootstrap` functions are pub and usable from
     fx-cli
   - May need to add standalone bootout/bootstrap functions that take a
     path (current `install`/`uninstall` do more than just load/unload)

## Tests

1. `stop_with_launchagent_calls_bootout` — mock: launchagent installed+loaded
   → verify bootout called, process NOT directly killed
2. `stop_without_launchagent_uses_sigterm` — mock: no launchagent → verify
   current behavior preserved
3. `start_with_plist_calls_bootstrap` — mock: plist exists → verify
   bootstrap called, no direct spawn
4. `start_without_plist_spawns_directly` — mock: no plist → verify current
   behavior

## Branch

`fix/daemon-aware-lifecycle` from `origin/dev`
PR targets `dev`.
