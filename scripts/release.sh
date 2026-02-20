#!/usr/bin/env bash
#
# Citros Release Script
#
# Generates a fresh CITROS_APP_TOKEN, deploys it to Vercel, builds the APK
# with the matching token compiled in, and optionally installs on a connected
# device. Both server and client stay in sync — no manual copy-paste.
#
# Prerequisites:
#   - vercel CLI (npm i -g vercel) + logged in
#   - Android SDK / JDK 17 (JAVA_HOME, ANDROID_SDK_ROOT set)
#   - openssl
#   - adb (optional, for --install)
#
# Usage:
#   ./scripts/release.sh                    # build debug APK
#   ./scripts/release.sh --install          # build + adb install
#   ./scripts/release.sh --release          # build release APK
#   ./scripts/release.sh --token-only       # rotate token on Vercel, skip APK build
#   ./scripts/release.sh --no-rotate        # build APK with existing token (no rotation)
#   CITROS_APP_TOKEN=xxx ./scripts/release.sh --no-rotate  # use explicit token
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ANDROID_DIR="$REPO_ROOT/android"
LANDING_DIR="${CITROS_LANDING_DIR:-$REPO_ROOT/../citros-landing}"
TOKEN_FILE="$REPO_ROOT/.citros-app-token"

# Flags
INSTALL=false
RELEASE=false
TOKEN_ONLY=false
NO_ROTATE=false
DEVICE=""

for arg in "$@"; do
    case $arg in
        --install)   INSTALL=true ;;
        --release)   RELEASE=true ;;
        --token-only) TOKEN_ONLY=true ;;
        --no-rotate) NO_ROTATE=true ;;
        --device=*)  DEVICE="${arg#*=}" ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            echo "Unknown flag: $arg" >&2
            exit 1
            ;;
    esac
done

# ── Step 1: Token ─────────────────────────────────────────────────
if [ "$NO_ROTATE" = true ]; then
    if [ -n "${CITROS_APP_TOKEN:-}" ]; then
        echo "▸ Using provided CITROS_APP_TOKEN"
    elif [ -f "$TOKEN_FILE" ]; then
        CITROS_APP_TOKEN=$(cat "$TOKEN_FILE")
        echo "▸ Using cached token from $TOKEN_FILE"
    else
        echo "✗ --no-rotate requires CITROS_APP_TOKEN env var or $TOKEN_FILE" >&2
        exit 1
    fi
else
    CITROS_APP_TOKEN=$(openssl rand -hex 32)
    echo "▸ Generated new CITROS_APP_TOKEN"
fi

# Save token locally (git-ignored) for --no-rotate rebuilds
echo -n "$CITROS_APP_TOKEN" > "$TOKEN_FILE"
chmod 600 "$TOKEN_FILE"

# ── Step 2: Deploy token to Vercel ────────────────────────────────
echo "▸ Updating Vercel environment..."

if ! command -v vercel &>/dev/null; then
    echo "✗ vercel CLI not found. Install: npm i -g vercel" >&2
    exit 1
fi

# Remove + re-add for both production and preview
for ENV in production preview; do
    vercel env rm CITROS_APP_TOKEN "$ENV" -y 2>/dev/null || true
    echo -n "$CITROS_APP_TOKEN" | vercel env add CITROS_APP_TOKEN "$ENV" 2>/dev/null
done
echo "  ✓ Token set for production + preview"

# Trigger production redeploy
echo "▸ Deploying to Vercel..."
if [ -d "$LANDING_DIR" ]; then
    (cd "$LANDING_DIR" && vercel --prod --yes 2>&1 | tail -3)
    echo "  ✓ Vercel production deployed"
else
    echo "  ⚠ Landing dir not found at $LANDING_DIR — skipping deploy"
    echo "    Set CITROS_LANDING_DIR or run 'vercel --prod' manually"
fi

if [ "$TOKEN_ONLY" = true ]; then
    echo ""
    echo "✓ Token rotated and deployed. Skipping APK build (--token-only)."
    exit 0
fi

# ── Step 3: Build APK ─────────────────────────────────────────────
echo "▸ Building APK..."

BUILD_TYPE="assembleDebug"
APK_PATH="chat/build/outputs/apk/debug/chat-debug.apk"
if [ "$RELEASE" = true ]; then
    BUILD_TYPE="assembleRelease"
    APK_PATH="chat/build/outputs/apk/release/chat-release.apk"
fi

cd "$ANDROID_DIR"

# Write token to local.properties (gitignored) instead of passing via CLI
# to avoid exposure in `ps aux` output and Gradle build scans.
LOCAL_PROPS="$ANDROID_DIR/local.properties"
if [ -f "$LOCAL_PROPS" ]; then
    # Remove existing citrosAppToken line if present
    sed -i.bak '/^citrosAppToken=/d' "$LOCAL_PROPS"
    rm -f "$LOCAL_PROPS.bak"
fi
echo "citrosAppToken=$CITROS_APP_TOKEN" >> "$LOCAL_PROPS"
chmod 600 "$LOCAL_PROPS"

./gradlew ":chat:$BUILD_TYPE" \
    --no-daemon \
    2>&1 | tail -5

FULL_APK="$ANDROID_DIR/$APK_PATH"
if [ ! -f "$FULL_APK" ]; then
    echo "✗ APK not found at $FULL_APK" >&2
    exit 1
fi

APK_SIZE=$(du -h "$FULL_APK" | cut -f1)
echo "  ✓ APK built: $APK_PATH ($APK_SIZE)"

# ── Step 4: Install (optional) ────────────────────────────────────
if [ "$INSTALL" = true ]; then
    echo "▸ Installing on device..."
    ADB_ARGS=()
    if [ -n "$DEVICE" ]; then
        ADB_ARGS=(-s "$DEVICE")
    fi
    adb "${ADB_ARGS[@]}" uninstall ai.citros.chat 2>/dev/null || true
    adb "${ADB_ARGS[@]}" install -r "$FULL_APK"
    echo "  ✓ Installed on device"
fi

# ── Done ──────────────────────────────────────────────────────────
echo ""
echo "✓ Release complete."
# Only show partial token in interactive terminals (not CI logs)
if [ -t 1 ]; then
    echo "  Token:  ${CITROS_APP_TOKEN:0:8}...${CITROS_APP_TOKEN: -4}"
fi
echo "  APK:    $FULL_APK"
[ "$INSTALL" = true ] && echo "  Device: installed"
