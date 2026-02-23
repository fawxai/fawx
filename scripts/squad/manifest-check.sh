#!/usr/bin/env bash
set -euo pipefail

MANIFEST="/tmp/squad-manifest.jsonl"
JSON_OUT=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--manifest)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --manifest requires a value" >&2
        exit 2
      fi
      MANIFEST="$2"; shift 2 ;;
    --json)
      JSON_OUT=1; shift ;;
    -h|--help)
      cat <<USAGE
Usage: scripts/squad/manifest-check.sh [--manifest <path>] [--json]

Validates squad manifest JSONL consistency and live-entry invariants.
Default manifest path: /tmp/squad-manifest.jsonl
USAGE
      exit 0 ;;
    *)
      echo "Unknown arg: $1" >&2
      exit 2 ;;
  esac
done

python3 - "$MANIFEST" "$JSON_OUT" <<'PY2'
import errno
import json
import os
import sys
from collections import defaultdict

manifest_path = sys.argv[1]
try:
    json_out = bool(int(sys.argv[2]))
except Exception:
    print("ERROR: --json expects internal 0/1 value")
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


def emit(result: dict, code: int):
    if json_out:
        print(json.dumps(result, indent=2))
    else:
        print(f"Manifest: {result['manifest']}")
        print(f"Entries: {result['entries']} total | {result['claimed_entries']} claimed")
        if result["warnings"]:
            print("\nWarnings:")
            for w in result["warnings"]:
                print(f"  - {w}")
        if result["errors"]:
            print("\nErrors:")
            for e in result["errors"]:
                print(f"  - {e}")
        if not result["errors"]:
            print("\nOK: manifest passed consistency checks")
    sys.exit(code)


if not os.path.exists(manifest_path):
    emit({
        "manifest": manifest_path,
        "entries": 0,
        "claimed_entries": 0,
        "warnings": [],
        "errors": [f"manifest not found: {manifest_path}"],
    }, 1)

entries = []
errors = []
warnings = []
required = ["id", "branch", "worktree", "log", "state"]

with open(manifest_path, "r", encoding="utf-8") as f:
    for ln_no, line in enumerate(f, start=1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            obj = json.loads(line)
        except Exception as e:
            errors.append(f"line {ln_no}: invalid JSON ({e})")
            continue

        for k in required:
            if k not in obj:
                errors.append(f"line {ln_no}: missing required key '{k}'")

        st = obj.get("state")
        if st not in ALLOWED_STATES:
            errors.append(f"line {ln_no}: invalid state '{st}' (allowed: {sorted(ALLOWED_STATES)})")

        pid, pid_err = parse_pid(obj.get("pid"))
        if pid_err:
            errors.append(f"line {ln_no}: {pid_err}")
        obj["_pid"] = pid
        obj["_line"] = ln_no
        entries.append(obj)

claimed = [e for e in entries if e.get("state") in CLAIMED_STATES]

for key in ("id", "branch", "worktree"):
    grouped = defaultdict(list)
    for e in claimed:
        grouped[str(e.get(key))].append(e)
    for val, arr in grouped.items():
        if len(arr) > 1:
            lines = ",".join(str(x["_line"]) for x in arr)
            errors.append(f"duplicate claimed {key}='{val}' at lines {lines}")

pids = defaultdict(list)
for e in claimed:
    st = e.get("state")
    pid = e.get("_pid")
    if st in PID_REQUIRED_STATES and pid is None:
        errors.append(f"line {e['_line']}: state '{st}' requires pid")
    if pid is not None:
        pids[pid].append(e)

for pid, arr in pids.items():
    if len(arr) > 1:
        lines = ",".join(str(x["_line"]) for x in arr)
        errors.append(f"duplicate claimed pid={pid} at lines {lines}")

for e in claimed:
    st = e.get("state")
    wt = e.get("worktree")
    lg = e.get("log")
    pid = e.get("_pid")

    if wt and not os.path.isdir(wt):
        errors.append(f"line {e['_line']}: worktree missing: {wt}")
    if lg and not os.path.exists(lg):
        warnings.append(f"line {e['_line']}: log file missing: {lg}")

    if st in PID_REQUIRED_STATES and pid is not None and not pid_alive(pid):
        warnings.append(f"line {e['_line']}: pid {pid} not running (state={st})")

result = {
    "manifest": manifest_path,
    "entries": len(entries),
    "claimed_entries": len(claimed),
    "warnings": warnings,
    "errors": errors,
}

emit(result, 1 if errors else 0)
PY2
