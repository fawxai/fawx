#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST_CHECK="$ROOT_DIR/scripts/squad/manifest-check.sh"
MONITOR="$ROOT_DIR/scripts/squad/monitor.sh"
MANIFEST_UPSERT="$ROOT_DIR/scripts/squad/manifest-upsert.sh"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

expect_exit() {
  local expected="$1"
  shift
  set +e
  "$@" >/tmp/squad-tools-test.out 2>&1
  local rc=$?
  set -e
  if [[ "$rc" -ne "$expected" ]]; then
    cat /tmp/squad-tools-test.out >&2 || true
    fail "expected exit $expected got $rc for: $*"
  fi
}

iso_now() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR" /tmp/squad-tools-test.out' EXIT

# Case 1: healthy running worker
log1="$TMP_DIR/healthy.log"
: > "$log1"
cat > "$TMP_DIR/healthy.jsonl" <<JSON
{"id":"1","branch":"fix/1","worktree":"$ROOT_DIR","log":"$log1","pid":$$,"state":"running","startedAt":"$(iso_now)"}
JSON
expect_exit 0 "$MANIFEST_CHECK" --manifest "$TMP_DIR/healthy.jsonl"
expect_exit 0 "$MONITOR" --manifest "$TMP_DIR/healthy.jsonl" --stall-minutes 60 --max-runtime-minutes 240

# Case 2: duplicate claimed id/branch/worktree
cat > "$TMP_DIR/dupe.jsonl" <<JSON
{"id":"dupe","branch":"fix/dupe","worktree":"$ROOT_DIR","log":"$log1","state":"queued"}
{"id":"dupe","branch":"fix/dupe","worktree":"$ROOT_DIR","log":"$log1","state":"queued"}
JSON
expect_exit 1 "$MANIFEST_CHECK" --manifest "$TMP_DIR/dupe.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/dupe.jsonl" --stall-minutes 60 --max-runtime-minutes 240

# Case 3: dead pid
cat > "$TMP_DIR/dead.jsonl" <<JSON
{"id":"dead","branch":"fix/dead","worktree":"$ROOT_DIR","log":"$log1","pid":999999,"state":"running","startedAt":"$(iso_now)"}
JSON
expect_exit 0 "$MANIFEST_CHECK" --manifest "$TMP_DIR/dead.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/dead.jsonl" --stall-minutes 60 --max-runtime-minutes 240

# Case 4: stalled log age
log2="$TMP_DIR/stalled.log"
: > "$log2"
python3 - <<PY
import os,time
os.utime("$log2", (time.time()-3600, time.time()-3600))
PY
cat > "$TMP_DIR/stalled.jsonl" <<JSON
{"id":"stalled","branch":"fix/stalled","worktree":"$ROOT_DIR","log":"$log2","pid":$$,"state":"running","startedAt":"$(iso_now)"}
JSON
expect_exit 0 "$MANIFEST_CHECK" --manifest "$TMP_DIR/stalled.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/stalled.jsonl" --stall-minutes 5 --max-runtime-minutes 240

# Case 5: overdue runtime
cat > "$TMP_DIR/overdue.jsonl" <<JSON
{"id":"overdue","branch":"fix/overdue","worktree":"$ROOT_DIR","log":"$log1","pid":$$,"state":"running","startedAt":"2020-01-01T00:00:00Z"}
JSON
expect_exit 0 "$MANIFEST_CHECK" --manifest "$TMP_DIR/overdue.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/overdue.jsonl" --stall-minutes 60 --max-runtime-minutes 10

# Case 6: queued with no pid should be valid and healthy
cat > "$TMP_DIR/queued.jsonl" <<JSON
{"id":"queued","branch":"fix/queued","worktree":"$ROOT_DIR","log":"$log1","state":"queued","startedAt":"$(iso_now)"}
JSON
expect_exit 0 "$MANIFEST_CHECK" --manifest "$TMP_DIR/queued.jsonl"
expect_exit 0 "$MONITOR" --manifest "$TMP_DIR/queued.jsonl" --stall-minutes 5 --max-runtime-minutes 10

# Case 7: invalid state
cat > "$TMP_DIR/invalid-state.jsonl" <<JSON
{"id":"bad","branch":"fix/bad","worktree":"$ROOT_DIR","log":"$log1","state":"runing","pid":$$}
JSON
expect_exit 1 "$MANIFEST_CHECK" --manifest "$TMP_DIR/invalid-state.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/invalid-state.jsonl"


# Case 8: invalid pid values must fail
cat > "$TMP_DIR/invalid-pid.jsonl" <<JSON
{"id":"badpid","branch":"fix/badpid","worktree":"$ROOT_DIR","log":"$log1","state":"running","pid":0,"startedAt":"$(iso_now)"}
JSON
expect_exit 1 "$MANIFEST_CHECK" --manifest "$TMP_DIR/invalid-pid.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/invalid-pid.jsonl"

# Case 9: missing log for running worker should alert monitor
cat > "$TMP_DIR/missing-log-running.jsonl" <<JSON
{"id":"nolog","branch":"fix/nolog","worktree":"$ROOT_DIR","log":"$TMP_DIR/does-not-exist.log","state":"running","pid":$$,"startedAt":"$(iso_now)"}
JSON
expect_exit 0 "$MANIFEST_CHECK" --manifest "$TMP_DIR/missing-log-running.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/missing-log-running.jsonl"

# Case 10: malformed JSON should fail monitor closed
cat > "$TMP_DIR/malformed.jsonl" <<'JSON'
{"id":"ok","branch":"fix/ok","worktree":"/tmp","log":"/tmp/x","state":"queued"}
{bad json
JSON
expect_exit 1 "$MANIFEST_CHECK" --manifest "$TMP_DIR/malformed.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/malformed.jsonl"

# Case 11: queued with invalid pid should alert fail-closed
cat > "$TMP_DIR/queued-invalid-pid.jsonl" <<JSON
{"id":"qbad","branch":"fix/qbad","worktree":"$ROOT_DIR","log":"$log1","state":"queued","pid":0}
JSON
expect_exit 1 "$MANIFEST_CHECK" --manifest "$TMP_DIR/queued-invalid-pid.jsonl"
expect_exit 2 "$MONITOR" --manifest "$TMP_DIR/queued-invalid-pid.jsonl"

# Case 12: missing option values should return usage error code 2
expect_exit 2 "$MONITOR" --stall-minutes
expect_exit 2 "$MONITOR" --max-runtime-minutes
expect_exit 2 "$MONITOR" --manifest

# Case 13: upsert should reject running/restarted states without pid
upsert_missing_pid_manifest="$TMP_DIR/upsert-missing-pid.jsonl"
expect_exit 1 "$MANIFEST_UPSERT" \
  --manifest "$upsert_missing_pid_manifest" \
  --id "missing-pid-running" \
  --branch "fix/missing-pid-running" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "running"

expect_exit 1 "$MANIFEST_UPSERT" \
  --manifest "$upsert_missing_pid_manifest" \
  --id "missing-pid-restarted" \
  --branch "fix/missing-pid-restarted" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "restarted"

# Case 14: upsert should reject invalid state values
expect_exit 1 "$MANIFEST_UPSERT" \
  --manifest "$TMP_DIR/upsert-invalid-state.jsonl" \
  --id "invalid-state" \
  --branch "fix/invalid-state" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "runing"

# Case 15: upsert should reject invalid restarts values
expect_exit 1 "$MANIFEST_UPSERT" \
  --manifest "$TMP_DIR/upsert-invalid-restarts-neg.jsonl" \
  --id "invalid-restarts-neg" \
  --branch "fix/invalid-restarts-neg" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "queued" \
  --restarts "-1"

expect_exit 1 "$MANIFEST_UPSERT" \
  --manifest "$TMP_DIR/upsert-invalid-restarts-text.jsonl" \
  --id "invalid-restarts-text" \
  --branch "fix/invalid-restarts-text" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "queued" \
  --restarts "not-a-number"

# Case 16: queued insert then running transition should update same id entry
upsert_manifest="$TMP_DIR/upsert-transition.jsonl"
cat > "$upsert_manifest" <<JSON
# preserved comment

{"id":"other","branch":"fix/other","worktree":"$ROOT_DIR","log":"$log1","state":"done"}
JSON

expect_exit 0 "$MANIFEST_UPSERT" \
  --manifest "$upsert_manifest" \
  --id "upsert-1" \
  --branch "fix/upsert-1" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "queued"

expect_exit 0 "$MANIFEST_UPSERT" \
  --manifest "$upsert_manifest" \
  --id "upsert-1" \
  --branch "fix/upsert-1" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "running" \
  --pid "$$" \
  --started-at "$(iso_now)"

expect_exit 0 "$MANIFEST_CHECK" --manifest "$upsert_manifest"
expect_exit 0 "$MONITOR" --manifest "$upsert_manifest" --stall-minutes 60 --max-runtime-minutes 240
python3 - "$upsert_manifest" <<'PY'
import json
import sys

path = sys.argv[1]
entries = []
comments = 0
blanks = 0
with open(path, "r", encoding="utf-8") as f:
    for raw in f:
        s = raw.strip()
        if not s:
            blanks += 1
            continue
        if s.startswith("#"):
            comments += 1
            continue
        entries.append(json.loads(s))

target = [e for e in entries if str(e.get("id")) == "upsert-1"]
if len(target) != 1:
    raise SystemExit(f"expected exactly one upsert-1 entry, got {len(target)}")
e = target[0]
if e.get("state") != "running":
    raise SystemExit(f"expected running state, got {e.get('state')!r}")
if int(e.get("pid", 0)) <= 0:
    raise SystemExit(f"expected positive pid, got {e.get('pid')!r}")
if not e.get("startedAt"):
    raise SystemExit("expected startedAt on running entry")
if comments < 1 or blanks < 1:
    raise SystemExit("expected comment and blank lines to be preserved")
PY

# Case 17: duplicate id entries should collapse to one after upsert
dupe_upsert_manifest="$TMP_DIR/upsert-dupe.jsonl"
cat > "$dupe_upsert_manifest" <<JSON
{"id":"collapse","branch":"fix/old-a","worktree":"$ROOT_DIR","log":"$log1","state":"queued"}
{"id":"collapse","branch":"fix/old-b","worktree":"$ROOT_DIR","log":"$log1","state":"queued","restarts":2}
{"id":"keep","branch":"fix/keep","worktree":"$ROOT_DIR","log":"$log1","state":"done"}
JSON

expect_exit 0 "$MANIFEST_UPSERT" \
  --manifest "$dupe_upsert_manifest" \
  --id "collapse" \
  --branch "fix/collapse" \
  --worktree "$ROOT_DIR" \
  --log "$log1" \
  --state "running" \
  --pid "$$" \
  --started-at "$(iso_now)"

expect_exit 0 "$MANIFEST_CHECK" --manifest "$dupe_upsert_manifest"
python3 - "$dupe_upsert_manifest" <<'PY'
import json
import sys

path = sys.argv[1]
entries = []
with open(path, "r", encoding="utf-8") as f:
    for raw in f:
        s = raw.strip()
        if not s or s.startswith("#"):
            continue
        entries.append(json.loads(s))

collapse = [e for e in entries if str(e.get("id")) == "collapse"]
if len(collapse) != 1:
    raise SystemExit(f"expected one collapse entry, got {len(collapse)}")
entry = collapse[0]
if entry.get("state") != "running":
    raise SystemExit(f"expected running state, got {entry.get('state')!r}")
if entry.get("restarts") != 2:
    raise SystemExit(f"expected restarts=2 carried from latest duplicate, got {entry.get('restarts')!r}")
PY

# Case 18: repeated upserts should keep file valid JSONL and avoid duplicate ids
repeated_manifest="$TMP_DIR/upsert-repeated.jsonl"
for i in $(seq 1 12); do
  expect_exit 0 "$MANIFEST_UPSERT" \
    --manifest "$repeated_manifest" \
    --id "repeated" \
    --branch "fix/repeated" \
    --worktree "$ROOT_DIR" \
    --log "$log1" \
    --state "running" \
    --pid "$$" \
    --started-at "$(iso_now)" \
    --restarts "$i"
done

expect_exit 0 "$MANIFEST_CHECK" --manifest "$repeated_manifest"
python3 - "$repeated_manifest" <<'PY'
import json
import sys

path = sys.argv[1]
ids = []
with open(path, "r", encoding="utf-8") as f:
    for ln, raw in enumerate(f, start=1):
        s = raw.strip()
        if not s or s.startswith("#"):
            continue
        obj = json.loads(s)
        ids.append(str(obj.get("id")))

if ids.count("repeated") != 1:
    raise SystemExit(f"expected exactly one repeated entry after repeated upserts, got {ids.count('repeated')}")
PY

# Case 19: concurrent writers with different ids should not lose updates
concurrent_manifest="$TMP_DIR/upsert-concurrent.jsonl"
for _ in $(seq 1 20); do
  : > "$concurrent_manifest"
  "$MANIFEST_UPSERT" \
    --manifest "$concurrent_manifest" \
    --id "concurrent-a" \
    --branch "fix/concurrent-a" \
    --worktree "$ROOT_DIR" \
    --log "$log1" \
    --state "queued" >/dev/null &
  pid_a=$!

  "$MANIFEST_UPSERT" \
    --manifest "$concurrent_manifest" \
    --id "concurrent-b" \
    --branch "fix/concurrent-b" \
    --worktree "$ROOT_DIR" \
    --log "$log1" \
    --state "queued" >/dev/null &
  pid_b=$!

  wait "$pid_a"
  wait "$pid_b"

  python3 - "$concurrent_manifest" <<'PY'
import json
import sys

path = sys.argv[1]
ids = []
with open(path, "r", encoding="utf-8") as f:
    for raw in f:
        s = raw.strip()
        if not s or s.startswith("#"):
            continue
        ids.append(str(json.loads(s).get("id")))

if ids.count("concurrent-a") != 1 or ids.count("concurrent-b") != 1:
    raise SystemExit(f"expected both concurrent ids exactly once, got {ids!r}")
PY
done

echo "PASS: squad manifest tools"
