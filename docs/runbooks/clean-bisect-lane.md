# Clean Bisect Lane Runbook

Use this when you need to test an older commit or branch in isolation without touching the main checkout, the normal `~/.fawx` session store, or the launchd-managed `8400` server.

This runbook captures the clean-lane workflow used for commit bisect testing of tool flailing and related regressions.

## Goals

- build server and macOS app from a specific commit in a detached worktree
- run the server on an isolated port
- use a fresh data dir with only the minimum auth/provider state copied in
- keep old sessions, experiments, and device state out of the test lane
- tear the lane down completely when finished

## Variables

Set these first:

```bash
COMMIT=e34dc733
LANE="bisect-$COMMIT"
PORT=8401

REPO=/Users/joseph/fawx
WORKTREE=/private/tmp/fawx-$LANE
TARGET_DIR=/Users/joseph/.cargo-targets/fawx-$LANE
DATA_DIR=/Users/joseph/.fawx-$LANE
DERIVED_DATA=/tmp/fawx-$LANE-macos
FAKE_XCTEST=/tmp/fawx-$LANE-fake.xctestconfiguration
APP_BUNDLE=$DERIVED_DATA/Build/Products/Debug/Fawx.app
```

Use a different `PORT` if `8401` is occupied.

## 1. Preflight

Confirm the commit exists and the port is free:

```bash
git -C "$REPO" rev-parse --verify "$COMMIT^{commit}"
lsof -nP -iTCP:$PORT -sTCP:LISTEN || true
```

If an older test lane still exists, remove it before continuing:

```bash
pkill -f "$TARGET_DIR/release/fawx serve --http --port $PORT --data-dir $DATA_DIR" || true
pkill -f "$APP_BUNDLE/Contents/MacOS/Fawx" || true
rm -rf "$DATA_DIR" "$TARGET_DIR" "$DERIVED_DATA" "$FAKE_XCTEST"
if test -d "$WORKTREE"; then
  git -C "$REPO" worktree remove --force "$WORKTREE"
fi
git -C "$REPO" worktree prune
```

## 2. Create the Detached Worktree

```bash
git -C "$REPO" worktree add -d "$WORKTREE" "$COMMIT"
git -C "$WORKTREE" rev-parse --short HEAD
```

## 3. Build the Server and macOS App

```bash
rm -rf "$TARGET_DIR" "$DERIVED_DATA"
mkdir -p "$TARGET_DIR"

CARGO_TARGET_DIR="$TARGET_DIR" \
  cargo build --release -p fx-cli --bin fawx \
  --manifest-path "$WORKTREE/engine/Cargo.toml"

xcodebuild \
  -project "$WORKTREE/app/Fawx.xcodeproj" \
  -scheme Fawx-macOS \
  -configuration Debug \
  -derivedDataPath "$DERIVED_DATA" \
  build
```

## 4. Seed a Fresh Data Dir

This copies only the minimum runtime state needed to make the server usable:

- `config.toml`
- `auth.db`
- `credentials.db`
- `.auth-salt`
- `.credentials-salt`
- `skills/`

It intentionally does **not** copy:

- `sessions.redb`
- `devices.json`
- `experiments/`
- `journal.jsonl`
- `bus.redb`
- `cron.redb`

Seed the lane and inject a fresh HTTP bearer token:

```bash
SRC=/Users/joseph/.fawx
HTTP_BEARER=$(openssl rand -hex 32)

rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"
cp "$SRC/config.toml" "$DATA_DIR/config.toml"
cp "$SRC/auth.db" "$DATA_DIR/auth.db"
cp "$SRC/credentials.db" "$DATA_DIR/credentials.db"
cp "$SRC/.auth-salt" "$DATA_DIR/.auth-salt"
cp "$SRC/.credentials-salt" "$DATA_DIR/.credentials-salt"
rsync -a "$SRC/skills/" "$DATA_DIR/skills/"

python - <<'PY' "$DATA_DIR/config.toml" "$PORT" "$HTTP_BEARER"
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
port = sys.argv[2]
token = sys.argv[3]
text = path.read_text()
text = re.sub(
    r"\[http\]\nport = \d+\nenabled = true(?:\nbearer_token = \".*?\")?",
    f'[http]\nport = {port}\nenabled = true\nbearer_token = "{token}"',
    text,
    count=1,
)
path.write_text(text)
PY
```

## 5. Start the Server

Run the server in its own terminal tab or background it with `nohup` if preferred:

```bash
"$TARGET_DIR/release/fawx" serve --http --port "$PORT" --data-dir "$DATA_DIR"
```

Verify the lane is healthy:

```bash
curl -fsS "http://127.0.0.1:$PORT/health"
```

## 6. Adopt a Local Device and Generate a Pairing Code

Create a local device token:

```bash
DEVICE_JSON=$(
  curl -fsS -X POST \
    -H "Authorization: Bearer $HTTP_BEARER" \
    -H 'Content-Type: application/json' \
    -d '{}' \
    "http://127.0.0.1:$PORT/v1/setup/adopt-local"
)

DEVICE_TOKEN=$(printf '%s' "$DEVICE_JSON" | python -c 'import json,sys; print(json.load(sys.stdin)["token"])')
```

Confirm the lane is empty:

```bash
curl -fsS \
  -H "Authorization: Bearer $DEVICE_TOKEN" \
  "http://127.0.0.1:$PORT/v1/sessions"
```

Generate a fallback pairing code:

```bash
curl -fsS -X POST \
  -H "Authorization: Bearer $DEVICE_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{}' \
  "http://127.0.0.1:$PORT/v1/pair/generate"
```

## 7. Launch the macOS App Against the Lane

Use isolated defaults/keychain overrides so the test app does not inherit the normal `8400` setup:

```bash
SUITE="ai.fawx.app.$LANE.$(date +%s)"
KEYCHAIN="ai.fawx.app.$LANE.$(date +%s)"

: > "$FAKE_XCTEST"
pkill -f "$APP_BUNDLE/Contents/MacOS/Fawx" || true

launchctl setenv XCTestConfigurationFilePath "$FAKE_XCTEST"
launchctl setenv FAWX_TEST_SERVER_URL "http://127.0.0.1:$PORT"
launchctl setenv FAWX_TEST_BEARER_TOKEN "$DEVICE_TOKEN"
launchctl setenv FAWX_TEST_PAIRED_DEVICE_NAME "Bisect $COMMIT"
launchctl setenv FAWX_TEST_DEFAULTS_SUITE "$SUITE"
launchctl setenv FAWX_TEST_KEYCHAIN_SERVICE "$KEYCHAIN"
launchctl setenv FAWX_TEST_DISABLE_LOCAL_INSTALL "1"

open -na "$APP_BUNDLE" --args --uitesting
sleep 5

launchctl unsetenv XCTestConfigurationFilePath
launchctl unsetenv FAWX_TEST_SERVER_URL
launchctl unsetenv FAWX_TEST_BEARER_TOKEN
launchctl unsetenv FAWX_TEST_PAIRED_DEVICE_NAME
launchctl unsetenv FAWX_TEST_DEFAULTS_SUITE
launchctl unsetenv FAWX_TEST_KEYCHAIN_SERVICE
launchctl unsetenv FAWX_TEST_DISABLE_LOCAL_INSTALL
```

Verify the live app is connected to the test server:

```bash
APP_PID=$(pgrep -f "$APP_BUNDLE/Contents/MacOS/Fawx" | tail -n 1)
ps -p "$APP_PID" -o pid=,etime=,command=
lsof -nP -a -p "$APP_PID" -iTCP
```

You should see established connections to `127.0.0.1:$PORT`.

## 8. Teardown

Stop the app, stop the server, remove the runtime state, then remove the worktree:

```bash
pkill -f "$APP_BUNDLE/Contents/MacOS/Fawx" || true
pkill -f "$TARGET_DIR/release/fawx serve --http --port $PORT --data-dir $DATA_DIR" || true

rm -rf "$DATA_DIR" "$TARGET_DIR" "$DERIVED_DATA" "$FAKE_XCTEST"

if test -d "$WORKTREE"; then
  git -C "$REPO" worktree remove --force "$WORKTREE"
fi
git -C "$REPO" worktree prune

lsof -nP -iTCP:$PORT -sTCP:LISTEN || true
```

## Notes

- Older commits may require the injected `[http].bearer_token` in `config.toml` before the HTTP API will boot on a blank data dir.
- If `setup/adopt-local` changes shape on a newer commit, inspect the response with `curl -i` once and adjust the JSON extraction.
- Do not point the lane at a cloned `~/.fawx` if you are trying to prove a regression on fresh state. Copy only the minimum auth/provider files listed above.
- If you need a second comparison lane, repeat the process with a different `LANE` and `PORT`.
