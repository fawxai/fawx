# Phase 2a: Persistent Log Files with Rotation

## Summary

Add file-based logging to `~/.fawx/logs/` with daily rotation and configurable retention. Currently Fawx logs to stderr only — logs are lost on restart, making debugging headless deployments, canary incidents, and post-mortem analysis impossible.

## Motivation

- **Headless mode** (`fawx serve`) runs as a daemon — stderr goes nowhere useful
- **Canary/ripcord incidents** need post-incident log analysis to understand what triggered
- **Telegram channel errors** are only visible in stderr during the session
- **Multi-node fleet** will need centralized log access per node
- Every production system needs persistent logs. This is overdue.

## Design

### Log Directory Structure

```
~/.fawx/
└── logs/
    ├── fawx.log              ← current day's log (symlink to dated file)
    ├── fawx.2026-03-09.log   ← rotated daily
    ├── fawx.2026-03-08.log
    └── fawx.2026-03-07.log
```

### Configuration

```toml
[logging]
# Enable file logging (default: true for serve mode, false for TUI mode)
file_logging = true

# Log level for file output (default: "info")
# Values: "error", "warn", "info", "debug", "trace"
file_level = "info"

# Log level for stderr output (default: "warn")
stderr_level = "warn"

# Maximum number of log files to keep (default: 7)
max_files = 7

# Log directory (default: "~/.fawx/logs")
# log_dir = "~/.fawx/logs"
```

### Log Format

```
2026-03-09T18:30:45.123Z INFO  [fx_kernel::loop_engine] cycle 3: reason phase complete (model=claude-sonnet-4-20250514, tokens=1,234)
2026-03-09T18:30:45.456Z INFO  [fx_tools] execute: read_file path="src/main.rs" (ok, 2.3ms)
2026-03-09T18:30:46.789Z WARN  [fx_cli::http_serve] request rejected: invalid bearer token (ip=127.0.0.1)
2026-03-09T18:30:47.012Z ERROR [fx_kernel::canary] signal threshold exceeded: error_rate=0.45 > baseline=0.12 (window=5min)
```

Format: `{timestamp} {level:5} [{target}] {message}`

- Timestamps: ISO 8601 with milliseconds, UTC
- Target: Rust module path (e.g., `fx_kernel::loop_engine`)
- No ANSI colors in file output
- Stderr retains current format (with colors when TTY)

### Rotation Strategy

- **Daily rotation** — new file at midnight UTC
- **Retention** — keep `max_files` days, delete older
- **Implementation** — `tracing-appender` crate with `RollingFileAppender::daily()`
- **Cleanup** — on startup, scan log dir, delete files older than retention period
- **No compression** — keeps it simple, logs are text and compress well if needed externally

## Implementation

### Crate: `tracing-appender`

Already in the Rust ecosystem, well-maintained, designed for this exact use case:

```rust
use tracing_appender::rolling::{RollingFileAppender, Rotation};

let file_appender = RollingFileAppender::builder()
    .rotation(Rotation::DAILY)
    .filename_prefix("fawx")
    .filename_suffix("log")
    .max_log_files(config.max_files)
    .build(log_dir)?;
```

### Files to Modify

1. **`engine/crates/fx-cli/Cargo.toml`**
   - Add `tracing-appender` dependency

2. **`engine/crates/fx-cli/src/startup.rs`**
   - New function: `init_logging(config: &LoggingConfig) -> Result<WorkerGuard>`
   - Set up multi-writer subscriber: stderr + file appender
   - Return `WorkerGuard` (must be held for duration — drop flushes)
   - Create `~/.fawx/logs/` directory if not exists
   - Run retention cleanup on startup

3. **`engine/crates/fx-cli/src/main.rs`**
   - Call `init_logging()` early in startup
   - Hold `WorkerGuard` in main scope

4. **`engine/crates/fx-cli/src/headless.rs`**
   - HeadlessApp startup calls `init_logging()` (for `fawx serve` mode)

5. **Config struct** (wherever FawxConfig lives)
   - Add `LoggingConfig` section with fields above
   - Defaults: file_logging=true (serve), file_level="info", stderr_level="warn", max_files=7

### Multi-Writer Setup

```rust
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter};

let file_layer = fmt::layer()
    .with_writer(file_appender)
    .with_ansi(false)
    .with_target(true)
    .with_timer(fmt::time::UtcTime::rfc_3339());

let stderr_layer = fmt::layer()
    .with_writer(std::io::stderr)
    .with_ansi(atty::is(atty::Stream::Stderr))
    .with_target(true);

let subscriber = tracing_subscriber::registry()
    .with(file_layer.with_filter(file_filter))
    .with(stderr_layer.with_filter(stderr_filter));

tracing::subscriber::set_global_default(subscriber)?;
```

### Tests Required

- Log file created in expected directory
- Daily rotation produces dated filenames
- Retention cleanup removes old files (mock filesystem or temp dir)
- Config defaults applied correctly
- Stderr output unaffected when file logging enabled
- File output has no ANSI codes
- `WorkerGuard` drop flushes pending writes
- Log directory created if missing
- Config parsing for all log level values
- Disabled file logging produces no files

## Dependencies

- `tracing-appender` — new dependency (justified: purpose-built for this, maintained by tokio team, no transitive bloat)
- `tracing-subscriber` — likely already present (check)

## Size Estimate

~150-200 lines of implementation + ~100 lines of tests. Single PR.

## Notes

- `tracing-appender` uses a non-blocking writer with a background thread — zero impact on hot path
- The `WorkerGuard` pattern ensures logs are flushed even on panic
- Future: structured JSON log format option for log aggregation (not in this PR)
- Future: `/logs` slash command to tail recent logs from TUI (not in this PR)
