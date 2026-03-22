# Fix: Stale redb Lock Cleanup on Start/Stop

## Problem

When `fawx serve` is killed with SIGKILL (via `killall -9 fawx`), two
artifacts survive:

1. **Stale PID file** — `PidFileGuard::drop()` never runs, leaving
   `~/.fawx/fawx.pid` behind. `fawx start` then reports "already running"
   even though the process is dead.

2. **Stale redb lock files** — redb uses file-level locks. While the kernel
   releases fcntl locks on process exit, redb may leave `.lock` marker files
   or internal lock state that prevents reopening. The next `fawx serve`
   startup warns "Database already open. Cannot acquire lock." for
   `bus.redb`, `sessions.redb`, `cron.redb`, and the credential store.

### Current behavior
- `fawx stop`: SIGTERM → wait 5s → SIGKILL fallback → remove PID file ✓
- `fawx start`: checks PID file → spawns new process → waits for PID file ✓
- `killall -9 fawx`: process dies immediately, no cleanup runs ✗
- Next `fawx serve`: redb databases fail to open, critical features disabled ✗

## Fix (2 parts)

### Part 1: Stale lock cleanup in `fawx serve` startup

At the very start of `fawx serve` (in `main.rs`, before any database opens),
detect and clean up stale state:

```rust
fn cleanup_stale_state(data_dir: &Path) {
    // Remove stale PID file if the recorded process is dead
    let pid_file = data_dir.join("fawx.pid");
    if let Ok(Some(pid)) = restart::read_pid_file(&pid_file) {
        if !process_is_alive(pid) {
            let _ = std::fs::remove_file(&pid_file);
            tracing::info!(pid, "removed stale PID file from dead process");
        }
    }

    // Remove stale redb lock files
    let lock_patterns = ["bus.redb.lock", "sessions.redb.lock", "cron.redb.lock"];
    for pattern in &lock_patterns {
        let lock_path = data_dir.join(pattern);
        if lock_path.exists() {
            let _ = std::fs::remove_file(&lock_path);
            tracing::info!(path = %lock_path.display(), "removed stale redb lock file");
        }
    }
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let pid = match i32::try_from(pid) {
            Ok(p) => nix::unistd::Pid::from_raw(p),
            Err(_) => return false,
        };
        matches!(
            nix::sys::signal::kill(pid, None),
            Ok(()) | Err(nix::errno::Errno::EPERM)
        )
    }
    #[cfg(not(unix))]
    {
        false
    }
}
```

Call `cleanup_stale_state(&data_dir)` in `main.rs` right before
`create_serve_pid_file_guard()`.

### Part 2: Post-stop lock cleanup in `fawx stop`

After `execute_stop` confirms the process is dead (both SIGTERM and SIGKILL
paths), clean up any stale lock files:

In `commands/start_stop.rs`, after `terminate_process` and before returning
`StopOutcome::Stopped`:

```rust
fn cleanup_redb_locks(data_dir: &Path) {
    for name in &["bus.redb.lock", "sessions.redb.lock", "cron.redb.lock"] {
        let path = data_dir.join(name);
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => eprintln!("Cleaned up stale lock: {}", path.display()),
                Err(e) => eprintln!("Warning: failed to clean lock {}: {e}", path.display()),
            }
        }
    }
}
```

This requires passing `data_dir` into `execute_stop`. Update the function
signature and call sites.

Also need to detect the data_dir in the stop command. Use the same resolution
as `pid_file_path()`:

```rust
fn data_dir_for_cleanup() -> PathBuf {
    let base = startup::fawx_data_dir();
    startup::load_config()
        .map(|config| startup::configured_data_dir(&base, &config))
        .unwrap_or(base)
}
```

### Part 3: Verify redb lock file names

Check what redb actually creates. The lock mechanism may use the database
file itself (fcntl) rather than separate `.lock` files. If so, the issue
is different — redb might embed lock state in the database file header.

Look at redb source or test empirically:
```
ls -la ~/.fawx/*.redb* ~/.fawx/*.lock 2>/dev/null
```

If redb uses file-header locks (not separate files), the fix is:
- On startup, try to open each database
- If "already locked" error occurs AND the PID file is stale (process dead),
  the lock is stale — redb should release it on close/reopen
- Actually: fcntl locks are released by the kernel on process death. So if
  the process is truly dead, redb should be able to open the file.
- The real cause might be: `fawx stop` sends SIGTERM, the process starts
  shutting down but hasn't released the database yet, then `fawx start`
  spawns a new process that tries to open the database before the old
  process finishes its shutdown.

### Root cause: race between stop and start

The most likely bug: `fawx stop` returns success (PID file removed) but the
process hasn't fully exited yet — redb databases are still held. Then
`fawx start` spawns a new process that tries to open the same databases.

Fix: In `terminate_process`, after `wait_for_exit` returns true, add a
small additional delay or verify that the process's file descriptors are
closed. OR: in `fawx serve` startup, retry database opens with backoff:

```rust
fn open_database_with_retry(path: &Path, max_attempts: u32) -> Result<Database, Error> {
    for attempt in 0..max_attempts {
        match Database::open(path) {
            Ok(db) => return Ok(db),
            Err(e) if attempt < max_attempts - 1 && is_lock_error(&e) => {
                tracing::warn!(
                    path = %path.display(),
                    attempt,
                    "database locked, retrying in {}ms",
                    100 * (attempt + 1)
                );
                std::thread::sleep(Duration::from_millis(100 * u64::from(attempt + 1)));
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
```

This handles both the race condition AND genuine stale locks.

## Files to Change

1. `engine/crates/fx-cli/src/main.rs`
   - Add `cleanup_stale_state()` call before `create_serve_pid_file_guard()`
   - Import necessary functions

2. `engine/crates/fx-cli/src/commands/start_stop.rs`
   - Add `cleanup_redb_locks()` after process termination in `execute_stop`
   - Pass data_dir through the stop path

3. `engine/crates/fx-cli/src/startup.rs`
   - Add `open_database_with_retry()` helper
   - Use it for `SessionRegistry::open`, `CronStore::open`, credential store,
     and bus.redb opens
   - Or: export the data_dir resolution for use by start_stop

4. `engine/crates/fx-session/src/registry.rs`
   - Consider adding retry logic in `SessionRegistry::open` itself

## Tests

1. `cleanup_stale_state_removes_dead_pid_file` — write PID file with
   non-existent PID, call cleanup, verify removed
2. `cleanup_stale_state_preserves_live_pid_file` — write PID file with
   current PID, call cleanup, verify preserved
3. `stop_cleans_up_lock_files_after_termination` — mock stop flow,
   verify lock file cleanup called
4. `serve_startup_retries_locked_database` — open database, hold lock,
   verify retry succeeds after release

## Branch

`fix/stale-redb-locks` from `origin/dev`
PR targets `dev`.
