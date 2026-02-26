#!/usr/bin/env bash
set -euo pipefail

MANIFEST="/tmp/squad-manifest.jsonl"
STALL_MINUTES=15
MAX_RUNTIME_MINUTES=180
JSON_OUT=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--manifest)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --manifest requires a value" >&2
        exit 2
      fi
      MANIFEST="$2"; shift 2 ;;
    --stall-minutes)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --stall-minutes requires a value" >&2
        exit 2
      fi
      STALL_MINUTES="$2"; shift 2 ;;
    --max-runtime-minutes)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --max-runtime-minutes requires a value" >&2
        exit 2
      fi
      MAX_RUNTIME_MINUTES="$2"; shift 2 ;;
    --json)
      JSON_OUT=1; shift ;;
    -h|--help)
      cat <<USAGE
Usage: scripts/squad/monitor.sh [--manifest <path>] [--stall-minutes 15] [--max-runtime-minutes 180] [--json]

Checks health status for claimed squad workers from manifest.
- Exit 0: no alerts
- Exit 2: one or more alerts (dead/stalled/overdue/missing_pid/missing_log/invalid_state)
USAGE
      exit 0 ;;
    *)
      echo "Unknown arg: $1" >&2
      exit 2 ;;
  esac
done

python3 - "$MANIFEST" "$STALL_MINUTES" "$MAX_RUNTIME_MINUTES" "$JSON_OUT" <<'PY2'
import errno
import json
import os
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone

manifest_path = sys.argv[1]
try:
    stall_minutes = int(sys.argv[2])
except Exception:
    print(f"ERROR: --stall-minutes must be an integer, got {sys.argv[2]!r}")
    sys.exit(2)
try:
    max_runtime_minutes = int(sys.argv[3])
except Exception:
    print(f"ERROR: --max-runtime-minutes must be an integer, got {sys.argv[3]!r}")
    sys.exit(2)
try:
    json_out = bool(int(sys.argv[4]))
except Exception:
    print("ERROR: --json expects internal 0/1 value")
    sys.exit(2)

if stall_minutes < 1:
    print("ERROR: --stall-minutes must be >= 1")
    sys.exit(2)
if max_runtime_minutes < 1:
    print("ERROR: --max-runtime-minutes must be >= 1")
    sys.exit(2)

ALLOWED_STATES = {"queued", "running", "restarted", "done", "failed", "canceled"}
CLAIMED_STATES = {"queued", "running", "restarted"}
PID_REQUIRED_STATES = {"running", "restarted"}


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError as e:
        if getattr(e, "errno", None) == errno.ESRCH:
            return False
        return True


def parse_pid(raw):
    if raw is None or raw == "":
        return None, None
    try:
        pid = int(raw)
    except Exception:
        return None, f"pid is not an int ({raw!r})"
    if pid <= 0:
        return None, f"pid must be > 0 ({raw!r})"
    return pid, None


def parse_started_at(raw):
    if not isinstance(raw, str) or not raw:
        return None
    try:
        if raw.endswith("Z"):
            return datetime.fromisoformat(raw.replace("Z", "+00:00"))
        return datetime.fromisoformat(raw)
    except Exception:
        return None


def emit_text(rows, alerts, warnings):
    print(f"Manifest: {manifest_path}")
    print(f"Claimed workers: {len(rows)}")
    print("id\tstate\tpid\truntime_m\tlog_age_m\tstatus")
    for r in rows:
        print(f"{r['id']}\t{r['state']}\t{r['pid_text']}\t{r['runtime_text']}\t{r['log_age_text']}\t{r['status']}")
    if warnings:
        print("\nWarnings:")
        for w in warnings:
            print(f"  - {w}")
    if alerts:
        print("\nALERTS:")
        for a in alerts:
            print(f"  - {a}")
        return 2
    print("\nOK: all claimed workers healthy")
    return 0


if not os.path.exists(manifest_path):
    msg = f"manifest not found: {manifest_path}"
    if json_out:
        print(json.dumps({"manifest": manifest_path, "error": msg}, indent=2))
    else:
        print(f"ERROR: {msg}")
    sys.exit(1)

entries = []
warnings = []
alerts = []
parse_errors = []

with open(manifest_path, "r", encoding="utf-8") as f:
    for ln_no, line in enumerate(f, start=1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            obj = json.loads(line)
            obj["_line"] = ln_no
            entries.append(obj)
        except Exception as e:
            parse_errors.append(f"line {ln_no}: invalid JSON ({e})")

if parse_errors:
    alerts.extend(parse_errors)

# include invalid-state entries so we can alert them explicitly
claimed = [e for e in entries if e.get("state") in CLAIMED_STATES or e.get("state") not in ALLOWED_STATES]

# duplicate claimed guards in monitor too (fail-closed when monitor is used standalone)
for key in ("id", "branch", "worktree"):
    grouped = defaultdict(list)
    for e in claimed:
        grouped[str(e.get(key))].append(e)
    for val, arr in grouped.items():
        if len(arr) > 1:
            lines = ",".join(str(x.get("_line")) for x in arr)
            alerts.append(f"duplicate claimed {key}='{val}' at lines {lines}")

pid_group = defaultdict(list)
for e in claimed:
    pid, pid_err = parse_pid(e.get("pid"))
    e["_pid"] = pid
    e["_pid_err"] = pid_err
    if pid is not None:
        pid_group[pid].append(e)
for pid, arr in pid_group.items():
    if len(arr) > 1:
        lines = ",".join(str(x.get("_line")) for x in arr)
        alerts.append(f"duplicate claimed pid={pid} at lines {lines}")

now = time.time()
rows = []

for e in claimed:
    wid = str(e.get("id"))
    state = e.get("state")
    pid = e.get("_pid")
    pid_err = e.get("_pid_err")

    runtime_m = None
    dt = parse_started_at(e.get("startedAt"))
    if dt is not None:
        runtime_m = (datetime.now(timezone.utc) - dt.astimezone(timezone.utc)).total_seconds() / 60

    log_age_m = None
    log = e.get("log")
    log_exists = isinstance(log, str) and os.path.exists(log)
    if log_exists:
        log_age_m = (now - os.path.getmtime(log)) / 60

    status = "healthy"
    if state not in ALLOWED_STATES:
        status = "invalid_state"
    elif pid_err:
        # if pid field is present but invalid for any state, fail closed
        status = "invalid_pid"
    elif state == "queued":
        # queued may omit pid entirely, but invalid provided pid is rejected above
        status = "queued"
    elif state in PID_REQUIRED_STATES:
        if pid is None:
            status = "missing_pid"
        elif not log_exists:
            status = "missing_log"
        elif not pid_alive(pid):
            status = "dead"
        elif log_age_m is not None and log_age_m > stall_minutes:
            status = "stalled"
        elif runtime_m is not None and runtime_m > max_runtime_minutes:
            status = "overdue"

    if status in {"invalid_state", "invalid_pid", "missing_pid", "missing_log", "dead", "stalled", "overdue"}:
        alerts.append(f"{wid}: {status}")

    # keep lightweight warnings for odd but non-alert conditions
    if state in PID_REQUIRED_STATES and dt is None:
        warnings.append(f"{wid}: startedAt missing/invalid")

    rows.append(
        {
            "id": wid,
            "state": state,
            "pid": pid,
            "pid_text": str(pid) if pid is not None else "-",
            "runtime_m": runtime_m,
            "runtime_text": f"{runtime_m:.1f}" if runtime_m is not None else "-",
            "log_age_m": log_age_m,
            "log_age_text": f"{log_age_m:.1f}" if log_age_m is not None else "-",
            "status": status,
            "line": e.get("_line"),
        }
    )

exit_code = 2 if alerts else 0

if json_out:
    print(json.dumps({
        "manifest": manifest_path,
        "stall_minutes": stall_minutes,
        "max_runtime_minutes": max_runtime_minutes,
        "claimed_count": len(rows),
        "rows": rows,
        "warnings": warnings,
        "alerts": alerts,
        "ok": not bool(alerts),
    }, indent=2))
    sys.exit(exit_code)

sys.exit(emit_text(rows, alerts, warnings))
PY2
