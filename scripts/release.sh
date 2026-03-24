#!/usr/bin/env bash
#
# Fawx Release Pipeline
#
# One-command release: bump version, build signed/notarized DMG, upload to
# GitHub Releases, update Sparkle appcast, deploy to fawx.ai.
#
# Must run on macOS with Xcode, signing identity, and notarytool configured.
#
# Prerequisites:
#   - macOS with Xcode + Apple Developer certificate
#   - gh CLI authenticated (for GitHub Releases)
#   - Sparkle sign_update tool (auto-discovered from DerivedData)
#   - scripts/build-dmg-config.sh (signing identity, notarize profile)
#   - fawx-site repo cloned alongside this repo (or set FAWX_SITE_DIR)
#
# Usage:
#   ./scripts/release.sh 1.2.0              # Full release
#   ./scripts/release.sh 1.2.0 --dry-run    # Show what would happen
#   ./scripts/release.sh 1.2.0 --skip-build # Skip DMG build (reuse existing)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$REPO_ROOT/build"
APP_DIR="$REPO_ROOT/app"
INFO_PLIST="$APP_DIR/Fawx/Info.plist"
INFO_PLIST_IOS="$APP_DIR/Fawx/Info-iOS.plist"
FAWX_SITE_DIR="${FAWX_SITE_DIR:-$REPO_ROOT/../fawx-site}"
RELEASE_REPO="abbudjoe/fawx-site"
DMG_PATH="$BUILD_DIR/Fawx.dmg"

# Flags
DRY_RUN=false
SKIP_BUILD=false

# ── Parse args ────────────────────────────────────────────────────
VERSION="${1:-}"
shift || true

for arg in "$@"; do
    case "$arg" in
        --dry-run)    DRY_RUN=true ;;
        --skip-build) SKIP_BUILD=true ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *) echo "Unknown flag: $arg" >&2; exit 1 ;;
    esac
done

if [[ -z "$VERSION" ]]; then
    echo "Usage: $0 <version> [--dry-run] [--skip-build]"
    echo "Example: $0 1.2.0"
    exit 1
fi

# Validate semver format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be semver (e.g. 1.2.0), got: $VERSION" >&2
    exit 1
fi

TAG="v$VERSION"

# ── Preflight checks ─────────────────────────────────────────────
echo "🦊 Fawx Release Pipeline — $TAG"
echo ""

# Must be on main branch
BRANCH="$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" ]]; then
    echo "Error: Must be on 'main' branch (currently on '$BRANCH')" >&2
    exit 1
fi

# Check for uncommitted changes
if ! git -C "$REPO_ROOT" diff --quiet HEAD; then
    echo "Error: Uncommitted changes on main. Commit or stash first." >&2
    exit 1
fi

# Verify fawx-site exists
if [[ ! -d "$FAWX_SITE_DIR/.git" ]]; then
    echo "Error: fawx-site repo not found at $FAWX_SITE_DIR" >&2
    echo "Clone it or set FAWX_SITE_DIR" >&2
    exit 1
fi

# Verify gh CLI
if ! command -v gh &>/dev/null; then
    echo "Error: gh CLI not found. Install: brew install gh" >&2
    exit 1
fi

# Check tag doesn't exist
if git -C "$REPO_ROOT" tag -l "$TAG" | grep -q "$TAG"; then
    echo "Error: Tag $TAG already exists" >&2
    exit 1
fi

echo "   Branch:    $BRANCH"
echo "   Version:   $VERSION"
echo "   Tag:       $TAG"
echo "   DMG repo:  $RELEASE_REPO"
echo "   Site dir:  $FAWX_SITE_DIR"
echo ""

if $DRY_RUN; then
    echo "── DRY RUN — no changes will be made ──"
    echo ""
fi

# ── Step 1: Bump version in Info.plist ────────────────────────────
bump_plist_version() {
    local plist="$1"
    local label="$2"

    # Read current build number, increment
    local current_build
    current_build=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "$plist" 2>/dev/null || echo "0")
    local new_build=$((current_build + 1))

    echo "   $label: $VERSION (build $new_build)"

    if ! $DRY_RUN; then
        /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$plist"
        /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $new_build" "$plist"
    fi
}

echo "── Step 1: Version bump ──"
bump_plist_version "$INFO_PLIST" "macOS"
bump_plist_version "$INFO_PLIST_IOS" "iOS"
echo ""

# Commit version bump
if ! $DRY_RUN; then
    git -C "$REPO_ROOT" add "$INFO_PLIST" "$INFO_PLIST_IOS"
    git -C "$REPO_ROOT" commit -m "release: bump version to $VERSION"
fi

# ── Step 2: Build DMG ────────────────────────────────────────────
echo "── Step 2: Build DMG ──"
if $SKIP_BUILD; then
    echo "   Skipping build (--skip-build)"
    if [[ ! -f "$DMG_PATH" ]]; then
        echo "Error: No DMG found at $DMG_PATH" >&2
        exit 1
    fi
elif $DRY_RUN; then
    echo "   Would run: scripts/build-dmg.sh --release"
else
    "$SCRIPT_DIR/build-dmg.sh" --release
fi

if [[ -f "$DMG_PATH" ]]; then
    DMG_SIZE=$(stat -f%z "$DMG_PATH" 2>/dev/null || stat -c%s "$DMG_PATH" 2>/dev/null)
    DMG_SIZE_HUMAN=$(du -h "$DMG_PATH" | cut -f1)
    echo "   DMG: $DMG_PATH ($DMG_SIZE_HUMAN)"
else
    DMG_SIZE=0
    echo "   DMG: (dry run, no file)"
fi
echo ""

# ── Step 3: EdDSA signature ──────────────────────────────────────
echo "── Step 3: Sparkle EdDSA signature ──"
EDDSA_SIGNATURE=""

find_sparkle_tool() {
    local tool_name="$1"
    if [[ -n "${FAWX_SPARKLE_TOOLS_DIR:-}" && -x "${FAWX_SPARKLE_TOOLS_DIR%/}/$tool_name" ]]; then
        echo "${FAWX_SPARKLE_TOOLS_DIR%/}/$tool_name"
        return 0
    fi
    for search_root in "$BUILD_DIR/derived/SourcePackages" "$HOME/Library/Developer/Xcode/DerivedData"; do
        [[ -d "$search_root" ]] || continue
        local found
        found="$(find "$search_root" -path "*/artifacts/sparkle/Sparkle/bin/$tool_name" -print -quit 2>/dev/null)"
        if [[ -n "$found" ]]; then
            echo "$found"
            return 0
        fi
    done
    return 1
}

if ! $DRY_RUN && [[ -f "$DMG_PATH" ]]; then
    SIGN_TOOL="$(find_sparkle_tool sign_update || true)"
    if [[ -n "$SIGN_TOOL" ]]; then
        SIGN_OUTPUT="$("$SIGN_TOOL" "$DMG_PATH" 2>&1)"
        # sign_update outputs: sparkle:edSignature="..." length="..."
        EDDSA_SIGNATURE="$(echo "$SIGN_OUTPUT" | grep -oP 'sparkle:edSignature="\K[^"]+' || true)"
        if [[ -n "$EDDSA_SIGNATURE" ]]; then
            echo "   ✅ Signature: ${EDDSA_SIGNATURE:0:20}..."
        else
            echo "   ⚠ sign_update ran but couldn't parse signature"
            echo "   Output: $SIGN_OUTPUT"
        fi
    else
        echo "   ⚠ sign_update tool not found; build the app first to fetch Sparkle"
    fi
else
    echo "   (skipped in dry run)"
fi
echo ""

# ── Step 4: Tag + push ───────────────────────────────────────────
echo "── Step 4: Tag release ──"
if $DRY_RUN; then
    echo "   Would tag: $TAG"
    echo "   Would push: main + tags"
else
    git -C "$REPO_ROOT" tag -a "$TAG" -m "Release $VERSION"
    git -C "$REPO_ROOT" push origin main --tags
    echo "   ✅ Tagged $TAG and pushed"
fi
echo ""

# ── Step 5: GitHub Release ───────────────────────────────────────
echo "── Step 5: GitHub Release ──"
DOWNLOAD_URL="https://github.com/$RELEASE_REPO/releases/download/$TAG/Fawx.dmg"

if $DRY_RUN; then
    echo "   Would create release $TAG on $RELEASE_REPO"
    echo "   Would upload: $DMG_PATH"
elif [[ -f "$DMG_PATH" ]]; then
    gh release create "$TAG" "$DMG_PATH" \
        --repo "$RELEASE_REPO" \
        --title "Fawx $TAG" \
        --notes "Fawx $VERSION for macOS 14.0+" \
        --latest
    echo "   ✅ Release created: $DOWNLOAD_URL"
else
    echo "   ⚠ No DMG file; skipping upload"
fi
echo ""

# ── Step 6: Update appcast.xml ───────────────────────────────────
echo "── Step 6: Update appcast.xml ──"
APPCAST_FILE="$FAWX_SITE_DIR/appcast.xml"

if [[ ! -f "$APPCAST_FILE" ]]; then
    echo "Error: appcast.xml not found at $APPCAST_FILE" >&2
    exit 1
fi

PUB_DATE="$(date -u '+%a, %d %b %Y %H:%M:%S %z')"

# Build the new <item> entry
NEW_ITEM="    <item>
      <title>Version $VERSION</title>
      <sparkle:version>$(( $(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "$INFO_PLIST" 2>/dev/null || echo 1) ))</sparkle:version>
      <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>14.0</sparkle:minimumSystemVersion>
      <pubDate>$PUB_DATE</pubDate>
      <enclosure
        url=\"$DOWNLOAD_URL\"
        sparkle:edSignature=\"${EDDSA_SIGNATURE:-SIGNATURE_PENDING}\"
        length=\"${DMG_SIZE:-0}\"
        type=\"application/octet-stream\" />
    </item>"

if $DRY_RUN; then
    echo "   Would insert entry for $VERSION into appcast.xml"
    echo "   Preview:"
    echo "$NEW_ITEM" | sed 's/^/     /'
else
    # Insert new item after the <language> line (before existing items or closing comment)
    # Use awk to insert after the last metadata line in <channel>
    awk -v new_item="$NEW_ITEM" '
    /^[[:space:]]*<language>/ {
        print
        getline
        # Print any comment/blank lines between <language> and first <item>
        while (/^[[:space:]]*<!--/ || /^[[:space:]]*$/) {
            print
            if (!getline) break
        }
        # Now insert the new item before the first <item> or </channel>
        print new_item
        print $0
        next
    }
    { print }
    ' "$APPCAST_FILE" > "$APPCAST_FILE.tmp"
    mv "$APPCAST_FILE.tmp" "$APPCAST_FILE"
    echo "   ✅ Appcast updated with $VERSION entry"
fi
echo ""

# ── Step 7: Deploy appcast to fawx.ai ────────────────────────────
echo "── Step 7: Deploy to fawx.ai ──"
if $DRY_RUN; then
    echo "   Would commit + push appcast.xml to fawx-site"
    echo "   Vercel auto-deploys on push"
else
    cd "$FAWX_SITE_DIR"
    git add appcast.xml
    git commit -m "release: appcast $VERSION"
    git push origin main
    echo "   ✅ Pushed to fawx-site; Vercel will auto-deploy"
fi
echo ""

# ── Done ──────────────────────────────────────────────────────────
echo "🦊 Release $TAG complete!"
echo ""
echo "   Version:    $VERSION"
echo "   Tag:        $TAG"
echo "   DMG:        $DOWNLOAD_URL"
echo "   Appcast:    https://fawx.ai/appcast.xml"
if [[ -n "${DMG_SIZE_HUMAN:-}" ]]; then
    echo "   DMG size:   $DMG_SIZE_HUMAN"
fi
echo ""
echo "   Sparkle will pick up the update on next check."
