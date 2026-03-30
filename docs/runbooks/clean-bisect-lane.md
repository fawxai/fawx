# Clean Bisect Lane Runbook

Use this when you need to test a commit or branch in isolation without touching the main checkout, the normal `~/.fawx` session store, or the launchd-managed `8400` server.

This runbook supports two modes:

- **Headless API** — fast, scriptable, no UI. Good for regression testing tool behavior, profile detection, and completion contracts.
- **macOS App** — full UI. Required when testing rendering, setup wizard, streaming display, or anything visual.

Pick the mode that matches what you're verifying. Both share the same setup through Step 5.

---

## Variables

Set these first:

```bash
COMMIT=e34dc733           # commit or branch to test
LANE="bisect-$COMMIT"
PORT=8401

REPO=/Users/joseph/fawx
WORKTREE=/private/tmp/fawx-$LANE
TARGET_DIR=/Users/joseph/.cargo-targets/fawx-$LANE
DATA_DIR=/Users/joseph/.fawx-$LANE
```

For macOS App mode, also set:

```bash
DERIVED_DATA=/tmp/fawx-$LANE-macos
FAKE_XCTEST=/tmp/fawx-$LANE-fake.xctestconfiguration
APP_BUNDLE=$DERIVED_DATA/Build/Products/Debug/Fawx.app
```

Use a different `PORT` if `8401` is occupied.

---

## 1. Preflight

Confirm the commit exists and the port is free:

```bash
git -C "$REPO" rev-parse --verify "$COMMIT^{commit}"
lsof -nP -iTCP:$PORT -sTCP:LISTEN || true
```

If an older test lane still exists, remove it before continuing:

```bash
pkill -f "$TARGET_DIR/release/fawx serve --http --port $PORT --data-dir $DATA_DIR" || true
pkill -f "$APP_BUNDLE/Contents/MacOS/Fawx" 2>/dev/null || true
rm -rf "$DATA_DIR" "$TARGET_DIR" "$DERIVED_DATA" "$FAKE_XCTEST" 2>/dev/null
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

## 3. Build

### Headless API mode (server only)

```bash
rm -rf "$TARGET_DIR"
mkdir -p "$TARGET_DIR"

# Older commits use engine/Cargo.toml; newer commits use the root Cargo.toml.
# Check which exists and use that.
if [ -f "$WORKTREE/engine/Cargo.toml" ]; then
  MANIFEST="$WORKTREE/engine/Cargo.toml"
else
  MANIFEST="$WORKTREE/Cargo.toml"
fi

CARGO_TARGET_DIR="$TARGET_DIR" \
  cargo build --release -p fx-cli --bin fawx \
  --manifest-path "$MANIFEST"
```

### macOS App mode (server + app)

```bash
rm -rf "$TARGET_DIR" "$DERIVED_DATA"
mkdir -p "$TARGET_DIR"

if [ -f "$WORKTREE/engine/Cargo.toml" ]; then
  MANIFEST="$WORKTREE/engine/Cargo.toml"
else
  MANIFEST="$WORKTREE/Cargo.toml"
fi

CARGO_TARGET_DIR="$TARGET_DIR" \
  cargo build --release -p fx-cli --bin fawx \
  --manifest-path "$MANIFEST"

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

## 5. Start the Server and Verify

Run the server in its own terminal tab or background it:

```bash
"$TARGET_DIR/release/fawx" serve --http --port "$PORT" --data-dir "$DATA_DIR"
```

Verify the lane is healthy:

```bash
curl -fsS "http://127.0.0.1:$PORT/health"
```

Adopt a local device:

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

---

## 6a. Headless API Testing

Use the headless API to send messages and verify behavior without a UI. This is the fast path for regression testing.

### Send a test message

```bash
SESSION_JSON=$(
  curl -fsS -X POST \
    -H "Authorization: Bearer $DEVICE_TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"message": "Read ~/.zshrc and tell me exactly what it says."}' \
    "http://127.0.0.1:$PORT/v1/chat"
)

echo "$SESSION_JSON" | python -m json.tool
```

### Check headless signals

The server writes structured signals to the data dir. After a test message:

```bash
cat "$DATA_DIR"/sessions/*/headless.jsonl | tail -20
```

### Example: Direct inspection regression test battery

```bash
run_test() {
  local label="$1" prompt="$2"
  echo "=== $label ==="
  RESULT=$(curl -fsS -X POST \
    -H "Authorization: Bearer $DEVICE_TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"message\": \"$prompt\"}" \
    "http://127.0.0.1:$PORT/v1/chat")
  echo "$RESULT" | python -c "
import json, sys
r = json.load(sys.stdin)
print(f\"iterations: {r.get('iterations', '?')}\")
text = r.get('response', r.get('text', ''))[:200]
print(f\"response: {text}\")
print()
"
}

run_test "T1: inspect local file" "Read ~/.zshrc and tell me exactly what it says."
run_test "T2: nonexistent file" "Read ~/.nonexistent_file_abc123 and show me the contents."
run_test "T3: standard continuation" "Read the README then make a small improvement to it."
run_test "T4: absolute path" "Read /etc/hosts and tell me what's in it."
```

### Pass criteria

| Test | Pass if |
|------|---------|
| T1 | iterations=1, response contains file content, no refusal |
| T2 | iterations=1, response acknowledges missing file, no hallucination |
| T3 | iterations>1, response shows read then edit/write activity |
| T4 | iterations=1, response contains file content, no working-dir refusal |

---

## 6b. macOS App Testing

Use this when you need to verify UI behavior, streaming rendering, setup flow, or visual regressions.

### Launch the app against the lane

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

### Verify the app is connected

```bash
APP_PID=$(pgrep -f "$APP_BUNDLE/Contents/MacOS/Fawx" | tail -n 1)
ps -p "$APP_PID" -o pid=,etime=,command=
lsof -nP -a -p "$APP_PID" -iTCP
```

You should see established connections to `127.0.0.1:$PORT`.

### UI smoke test checklist

- [ ] Setup wizard completes or skips correctly
- [ ] New session starts, message sends
- [ ] Streaming tokens appear incrementally
- [ ] Tool calls display in the tool panel
- [ ] Thinking indicator appears and resolves
- [ ] Session list updates
- [ ] No crashes or hangs

---

## 7. Teardown

Stop everything and clean up:

```bash
pkill -f "$APP_BUNDLE/Contents/MacOS/Fawx" 2>/dev/null || true
pkill -f "$TARGET_DIR/release/fawx serve --http --port $PORT --data-dir $DATA_DIR" || true

rm -rf "$DATA_DIR" "$TARGET_DIR" "$DERIVED_DATA" "$FAKE_XCTEST" 2>/dev/null

if test -d "$WORKTREE"; then
  git -C "$REPO" worktree remove --force "$WORKTREE"
fi
git -C "$REPO" worktree prune

lsof -nP -iTCP:$PORT -sTCP:LISTEN || true
```

---

## Notes

- Older commits may use `engine/Cargo.toml` as the manifest; newer commits use the root `Cargo.toml`. The build step checks for this automatically.
- If `setup/adopt-local` changes shape on a newer commit, inspect the response with `curl -i` once and adjust the JSON extraction.
- Do not point the lane at a cloned `~/.fawx` if you are trying to prove a regression on fresh state. Copy only the minimum auth/provider files listed above.
- If you need a second comparison lane, repeat the process with a different `LANE` and `PORT`.

## PR Integration Testing

Every PR that touches kernel behavior (loop engine, profiles, tool dispatch, completion logic) should run the headless API regression battery in Step 6a before the formal code review. For PRs that touch streaming, rendering, or the Swift app, also run the macOS app smoke test in Step 6b.

The test results (PASS/FAIL per test with key evidence) serve as the triage gate. Code review should not begin until triage passes.
