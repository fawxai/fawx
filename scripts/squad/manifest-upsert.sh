#!/usr/bin/env bash
set -euo pipefail

MANIFEST=""
ID=""
BRANCH=""
WORKTREE=""
LOG_PATH=""
STATE=""

HAS_PID=0
PID=""
HAS_STARTED_AT=0
STARTED_AT=""
HAS_RESTARTS=0
RESTARTS=""
JSON_OUT=0

usage() {
  cat <<USAGE
Usage: scripts/squad/manifest-upsert.sh --manifest <path> --id <id> --branch <branch> --worktree <path> --log <path> --state <state> [--pid <pid>] [--started-at <iso8601>] [--restarts <count>] [--json]

Atomically upserts one manifest JSONL record by id.
- Serializes concurrent writers with a per-manifest lock to avoid lost updates.
- Preserves comments/blank lines and unrelated entries.
- If duplicate ids are present, collapses them to one updated entry.
- States running/restarted require --pid on each upsert.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--manifest)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --manifest requires a value" >&2
        exit 2
      fi
      MANIFEST="$2"
      shift 2
      ;;
    --id)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --id requires a value" >&2
        exit 2
      fi
      ID="$2"
      shift 2
      ;;
    --branch)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --branch requires a value" >&2
        exit 2
      fi
      BRANCH="$2"
      shift 2
      ;;
    --worktree)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --worktree requires a value" >&2
        exit 2
      fi
      WORKTREE="$2"
      shift 2
      ;;
    --log)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --log requires a value" >&2
        exit 2
      fi
      LOG_PATH="$2"
      shift 2
      ;;
    --state)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --state requires a value" >&2
        exit 2
      fi
      STATE="$2"
      shift 2
      ;;
    --pid)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --pid requires a value" >&2
        exit 2
      fi
      HAS_PID=1
      PID="$2"
      shift 2
      ;;
    --started-at)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --started-at requires a value" >&2
        exit 2
      fi
      HAS_STARTED_AT=1
      STARTED_AT="$2"
      shift 2
      ;;
    --restarts)
      if [[ $# -lt 2 ]]; then
        echo "ERROR: --restarts requires a value" >&2
        exit 2
      fi
      HAS_RESTARTS=1
      RESTARTS="$2"
      shift 2
      ;;
    --json)
      JSON_OUT=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$MANIFEST" || -z "$ID" || -z "$BRANCH" || -z "$WORKTREE" || -z "$LOG_PATH" || -z "$STATE" ]]; then
  echo "ERROR: missing required flags" >&2
  usage >&2
  exit 2
fi

python3 - "$MANIFEST" "$ID" "$BRANCH" "$WORKTREE" "$LOG_PATH" "$STATE" "$HAS_PID" "$PID" "$HAS_STARTED_AT" "$STARTED_AT" "$HAS_RESTARTS" "$RESTARTS" "$JSON_OUT" <<'PY2'
import errno
import fcntl
import json
import os
import stat
import sys
import tempfile

manifest_path = sys.argv[1]
record_id = sys.argv[2]
branch = sys.argv[3]
worktree = sys.argv[4]
log_path = sys.argv[5]
state = sys.argv[6]
has_pid = bool(int(sys.argv[7]))
pid_raw = sys.argv[8]
has_started_at = bool(int(sys.argv[9]))
started_at = sys.argv[10]
has_restarts = bool(int(sys.argv[11]))
restarts_raw = sys.argv[12]
json_out = bool(int(sys.argv[13]))

ALLOWED_STATES = {"queued", "running", "restarted", "done", "failed", "canceled"}
PID_REQUIRED_STATES = {"running", "restarted"}


def fail(msg: str, code: int = 1) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(code)


if state not in ALLOWED_STATES:
    fail(f"invalid --state '{state}' (allowed: {sorted(ALLOWED_STATES)})")

if state in PID_REQUIRED_STATES and not has_pid:
    fail(f"state '{state}' requires --pid")

pid = None
if has_pid:
    try:
        pid = int(pid_raw)
    except Exception:
        fail(f"--pid must be an integer, got {pid_raw!r}")
    if pid <= 0:
        fail(f"--pid must be > 0, got {pid_raw!r}")

restarts = None
if has_restarts:
    try:
        restarts = int(restarts_raw)
    except Exception:
        fail(f"--restarts must be an integer, got {restarts_raw!r}")
    if restarts < 0:
        fail(f"--restarts must be >= 0, got {restarts_raw!r}")

if has_started_at and not started_at:
    fail("--started-at must not be empty when provided")

manifest_abs = os.path.abspath(manifest_path)
manifest_dir = os.path.dirname(manifest_abs) or "."
if not os.path.isdir(manifest_dir):
    fail(f"manifest directory does not exist: {manifest_dir}")

def fsync_directory_if_supported(path: str) -> None:
    ignored_errnos = {
        errno.EBADF,
        errno.EINVAL,
        errno.EPERM,
        errno.EACCES,
        errno.EROFS,
    }
    enotsup = getattr(errno, "ENOTSUP", None)
    if enotsup is not None:
        ignored_errnos.add(enotsup)

    flags = os.O_RDONLY
    odirectory = getattr(os, "O_DIRECTORY", 0)
    if odirectory:
        flags |= odirectory

    try:
        dir_fd = os.open(path, flags)
    except OSError as exc:
        if exc.errno in ignored_errnos:
            return
        raise

    try:
        try:
            os.fsync(dir_fd)
        except OSError as exc:
            if exc.errno in ignored_errnos:
                return
            raise
    finally:
        os.close(dir_fd)


lock_path = f"{manifest_abs}.lock"
try:
    lock_fd = os.open(lock_path, os.O_RDWR | os.O_CREAT, 0o600)
except OSError as exc:
    fail(f"unable to open manifest lock {lock_path}: {exc}")

tmp_path = None
try:
    fcntl.flock(lock_fd, fcntl.LOCK_EX)

    existing_lines = []
    existing_mode = None
    if os.path.exists(manifest_abs):
        st = os.stat(manifest_abs)
        existing_mode = stat.S_IMODE(st.st_mode)
        with open(manifest_abs, "r", encoding="utf-8") as f:
            existing_lines = f.readlines()

    updated_lines = []
    matched_entries = []
    insert_at = None

    for line_no, raw in enumerate(existing_lines, start=1):
        stripped = raw.strip()

        if not stripped or stripped.startswith("#"):
            updated_lines.append(raw)
            continue

        try:
            obj = json.loads(stripped)
        except Exception as exc:
            fail(f"line {line_no}: invalid JSON ({exc})")

        if not isinstance(obj, dict):
            fail(f"line {line_no}: expected JSON object")

        if str(obj.get("id")) == record_id:
            matched_entries.append(obj)
            if insert_at is None:
                insert_at = len(updated_lines)
            continue

        updated_lines.append(raw)

    base = matched_entries[-1] if matched_entries else {}
    entry = dict(base)
    entry.update(
        {
            "id": record_id,
            "branch": branch,
            "worktree": worktree,
            "log": log_path,
            "state": state,
        }
    )

    if has_pid:
        entry["pid"] = pid
    if has_started_at:
        entry["startedAt"] = started_at
    if has_restarts:
        entry["restarts"] = restarts

    entry_line = json.dumps(entry, separators=(",", ":")) + "\n"
    if insert_at is None:
        updated_lines.append(entry_line)
        action = "inserted"
    else:
        updated_lines.insert(insert_at, entry_line)
        action = "updated"

    duplicates_removed = max(0, len(matched_entries) - 1)

    fd, tmp_path = tempfile.mkstemp(prefix=".manifest-upsert.", suffix=".tmp", dir=manifest_dir, text=True)
    with os.fdopen(fd, "w", encoding="utf-8") as tmp:
        tmp.writelines(updated_lines)
        tmp.flush()
        os.fsync(tmp.fileno())

    if existing_mode is not None:
        os.chmod(tmp_path, existing_mode)

    os.replace(tmp_path, manifest_abs)
    fsync_directory_if_supported(manifest_dir)
finally:
    if tmp_path and os.path.exists(tmp_path):
        os.unlink(tmp_path)
    os.close(lock_fd)

result = {
    "manifest": manifest_abs,
    "id": record_id,
    "action": action,
    "duplicates_removed": duplicates_removed,
    "entry": entry,
}

if json_out:
    print(json.dumps(result, indent=2))
else:
    print(
        f"manifest upsert {action}: id={record_id} state={state} duplicates_removed={duplicates_removed} manifest={manifest_abs}"
    )
PY2
