# Squad Scripts

Utilities for `docs/runbooks/squad-v2.md`.

## Files

- `scripts/squad/manifest-upsert.sh`
  - Atomically upserts a single manifest entry by `id` (`manifest lock + temp file + rename`).
  - Preserves unrelated entries plus comment/blank lines.
  - Collapses duplicate `id` entries into one updated record.

- `scripts/squad/manifest-check.sh`
  - Validates manifest JSONL structure and claimed-worker consistency.
  - Catches duplicate claimed issue/branch/worktree/pid entries.
  - Validates state enum and per-state PID requirements.

- `scripts/squad/monitor.sh`
  - Health-checks claimed workers from manifest.
  - Flags invalid/missing_pid/missing_log/dead/stalled/overdue workers with exit code `2`.

## Manifest format (JSONL)

One JSON object per line, e.g.:

```json
{"id":"704","branch":"fix/704-focus-steal-cli","worktree":"/Users/clawdiobot/citros-squad-cli-704","log":"/tmp/codex-704.log","pid":48452,"state":"running","startedAt":"2026-02-23T02:16:00Z","restarts":0}
```

Required keys:
- `id`
- `branch`
- `worktree`
- `log`
- `state`

Supported states:
- `queued` (claimed, no PID required)
- `running` (PID required at upsert time)
- `restarted` (PID required at upsert time)
- `done`
- `failed`
- `canceled`

Recommended keys:
- `pid`
- `startedAt`
- `restarts`

## Upsert merge behavior

- When duplicate lines for the same `id` exist, upsert uses the most recent matching line as merge base and removes older duplicates.
- `id`, `branch`, `worktree`, `log`, and `state` are always overwritten from CLI flags.
- `pid`, `startedAt`, and `restarts` are overwritten only when `--pid`, `--started-at`, or `--restarts` are provided.
- Any other existing keys on the merge base record are preserved (legacy/custom metadata).

## Usage

```bash
# queue before launch
scripts/squad/manifest-upsert.sh \
  --manifest /tmp/squad-manifest.jsonl \
  --id 704 \
  --branch fix/704-focus-steal-cli \
  --worktree /Users/clawdiobot/citros-squad-cli-704 \
  --log /tmp/codex-704.log \
  --state queued

# transition same id to running after nohup launch (PID=$!)
scripts/squad/manifest-upsert.sh \
  --manifest /tmp/squad-manifest.jsonl \
  --id 704 \
  --branch fix/704-focus-steal-cli \
  --worktree /Users/clawdiobot/citros-squad-cli-704 \
  --log /tmp/codex-704.log \
  --state running \
  --pid "$PID" \
  --started-at "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

scripts/squad/manifest-check.sh --manifest /tmp/squad-manifest.jsonl
scripts/squad/monitor.sh --manifest /tmp/squad-manifest.jsonl --stall-minutes 15 --max-runtime-minutes 180
```

Monitor behavior notes:

- `queued` entries are allowed without PID.
- `running`/`restarted` require PID and log file.
- malformed JSONL lines are alerts (fail-closed).

Machine-readable output:

```bash
scripts/squad/manifest-upsert.sh --manifest /tmp/squad-manifest.jsonl --id 704 --branch fix/704-focus-steal-cli --worktree /Users/clawdiobot/citros-squad-cli-704 --log /tmp/codex-704.log --state queued --json
scripts/squad/manifest-check.sh --manifest /tmp/squad-manifest.jsonl --json
scripts/squad/monitor.sh --manifest /tmp/squad-manifest.jsonl --json
```

## Exit codes

- `manifest-upsert.sh`
  - `0`: upsert successful
  - `1`: invalid manifest content or invalid field values
  - `2`: invalid invocation

- `manifest-check.sh`
  - `0`: valid
  - `1`: invalid manifest/invariants

- `monitor.sh`
  - `0`: no alerts
  - `2`: one or more alerts
  - `1`: manifest missing/invalid invocation
