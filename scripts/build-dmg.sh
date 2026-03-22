#!/usr/bin/env bash
set -euo pipefail

# Fawx DMG Build Pipeline
# Builds the macOS .app bundle and packages it as a notarized DMG.
#
# Prerequisites:
#   - macOS with Xcode installed
#   - Apple Developer certificate in keychain
#   - xcrun notarytool configured (Apple ID + app-specific password)
#   - Rust toolchain (for engine binary)
#
# Usage:
#   ./scripts/build-dmg.sh                    # Build debug DMG
#   ./scripts/build-dmg.sh --release          # Build release DMG (signed + notarized)
#   ./scripts/build-dmg.sh --skip-notarize    # Build signed DMG without notarization
#   ./scripts/build-dmg.sh --background PATH  # Build DMG with a custom Finder background image

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONFIG_FILE="$SCRIPT_DIR/build-dmg-config.sh"
if [[ -f "$CONFIG_FILE" ]]; then
    # shellcheck disable=SC1090
    source "$CONFIG_FILE"
fi
APP_DIR="$REPO_ROOT/app"
BUILD_DIR="$REPO_ROOT/build"
APP_NAME="Fawx"
BUNDLE_ID="ai.fawx.app"
DMG_NAME="Fawx"
ENGINE_BINARY="fawx"
RELEASE=false
SKIP_NOTARIZE=false
SIGNING_IDENTITY="${FAWX_SIGNING_IDENTITY:-}"
NOTARIZE_PROFILE="${FAWX_NOTARIZE_PROFILE:-fawx-notarize}"
DMG_BACKGROUND_IMAGE="${FAWX_DMG_BACKGROUND_IMAGE:-}"
DEFAULT_DMG_BACKGROUND="$REPO_ROOT/assets/dmg-background.png"
SPARKLE_PUBLIC_KEY_PLACEHOLDER="PASTE_PUBLIC_KEY_HERE"

if [[ -z "$DMG_BACKGROUND_IMAGE" && -f "$DEFAULT_DMG_BACKGROUND" ]]; then
    DMG_BACKGROUND_IMAGE="$DEFAULT_DMG_BACKGROUND"
fi

# Parse args
while [[ $# -gt 0 ]]; do
    case "$1" in
        --release) RELEASE=true; shift ;;
        --skip-notarize) SKIP_NOTARIZE=true; shift ;;
        --identity) SIGNING_IDENTITY="$2"; shift 2 ;;
        --background) DMG_BACKGROUND_IMAGE="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

CARGO_PROFILE="debug"
CARGO_FLAGS=""
XCODE_CONFIG="Debug"
if $RELEASE; then
    CARGO_PROFILE="release"
    CARGO_FLAGS="--release --features fx-kernel/kernel-blind"
    XCODE_CONFIG="Release"
fi

echo "🦊 Fawx DMG Build Pipeline"
echo "   Mode: $(if $RELEASE; then echo 'Release (signed + notarized)'; else echo 'Debug'; fi)"
echo ""

plist_value() {
    local key="$1"
    /usr/libexec/PlistBuddy -c "Print :$key" "$APP_DIR/Fawx/Info.plist" 2>/dev/null || true
}

sparkle_updates_configured() {
    local feed_url public_key
    feed_url="$(plist_value SUFeedURL)"
    public_key="$(plist_value SUPublicEDKey)"

    [[ -n "$feed_url" && -n "$public_key" && "$public_key" != "$SPARKLE_PUBLIC_KEY_PLACEHOLDER" ]]
}

find_sparkle_tool() {
    local tool_name="$1"
    local search_root tool_path

    if [[ -n "${FAWX_SPARKLE_TOOLS_DIR:-}" && -x "${FAWX_SPARKLE_TOOLS_DIR%/}/$tool_name" ]]; then
        echo "${FAWX_SPARKLE_TOOLS_DIR%/}/$tool_name"
        return 0
    fi

    for search_root in \
        "$BUILD_DIR/derived/SourcePackages" \
        "$HOME/Library/Developer/Xcode/DerivedData"
    do
        [[ -d "$search_root" ]] || continue
        tool_path="$(find "$search_root" -path "*/artifacts/sparkle/Sparkle/bin/$tool_name" -print -quit 2>/dev/null)"
        if [[ -n "$tool_path" ]]; then
            echo "$tool_path"
            return 0
        fi
    done

    return 1
}

sign_dmg_for_sparkle() {
    local dmg_path="$1"
    local sign_tool
    local sign_output
    sign_tool="$(find_sparkle_tool sign_update || true)"

    if [[ -z "$sign_tool" ]]; then
        echo "   ⚠ Sparkle sign_update tool not found; skipping EdDSA signing"
        return 0
    fi

    echo "   Signing DMG for Sparkle..."
    if ! sign_output="$("$sign_tool" "$dmg_path" 2>&1)"; then
        printf '%s\n' "$sign_output" >&2
        return 1
    fi

    if [[ -n "$sign_output" ]]; then
        while IFS= read -r line; do
            echo "     $line"
        done <<< "$sign_output"
    fi
}

generate_sparkle_appcast() {
    local output_dir="$1"
    local appcast_tool
    appcast_tool="$(find_sparkle_tool generate_appcast || true)"

    if [[ -z "$appcast_tool" ]]; then
        echo "   ⚠ Sparkle generate_appcast tool not found; skipping appcast generation"
        return 0
    fi

    echo "   Generating Sparkle appcast..."
    "$appcast_tool" "$output_dir"
    echo "   ✅ Appcast: $output_dir/appcast.xml"
}

# Step 1: Build engine binary
step_engine() {
    echo "── Step 1/5: Building engine binary ──"
    cd "$REPO_ROOT"
    cargo build $CARGO_FLAGS
    local binary="$REPO_ROOT/target/$CARGO_PROFILE/$ENGINE_BINARY"
    if [[ ! -f "$binary" ]]; then
        echo "Error: Engine binary not found at $binary"
        exit 1
    fi
    echo "   ✅ Engine binary: $binary"
    echo ""
}

# Step 2: Build Swift app
step_swift() {
    echo "── Step 2/5: Building Swift app ──"
    cd "$APP_DIR"
    xcodebuild -project Fawx.xcodeproj \
        -scheme "Fawx-macOS" \
        -configuration "$XCODE_CONFIG" \
        -derivedDataPath "$BUILD_DIR/derived" \
        build \
        | tail -5
    echo "   ✅ Swift app built"
    echo ""
}

# Step 3: Assemble .app bundle
step_assemble() {
    echo "── Step 3/5: Assembling .app bundle ──"
    local app_bundle="$BUILD_DIR/$APP_NAME.app"
    local built_app
    built_app=$(find "$BUILD_DIR/derived" -name "$APP_NAME.app" -type d | head -1)
    if [[ -z "$built_app" ]]; then
        echo "Error: Built .app not found in derived data"
        exit 1
    fi

    # Copy built app to build dir
    rm -rf "$app_bundle"
    cp -R "$built_app" "$app_bundle"

    # Embed engine binary
    local macos_dir="$app_bundle/Contents/MacOS"
    cp "$REPO_ROOT/target/$CARGO_PROFILE/$ENGINE_BINARY" "$macos_dir/fawx-server"
    chmod +x "$macos_dir/fawx-server"

    echo "   ✅ App bundle: $app_bundle"
    echo "   ✅ Embedded engine: $macos_dir/fawx-server"
    echo ""
}

# Step 4: Code sign
step_sign() {
    echo "── Step 4/5: Code signing ──"
    local app_bundle="$BUILD_DIR/$APP_NAME.app"

    if [[ -z "$SIGNING_IDENTITY" ]]; then
        if $RELEASE; then
            echo "Error: --identity or FAWX_SIGNING_IDENTITY required for release builds"
            exit 1
        fi
        echo "   ⚠ No signing identity — ad-hoc signing for debug build"
        codesign --force --deep --sign - "$app_bundle"
    else
        codesign --force --deep --options runtime \
            --sign "$SIGNING_IDENTITY" \
            --entitlements "$APP_DIR/Fawx/Fawx.entitlements" \
            "$app_bundle" 2>/dev/null || \
        codesign --force --deep --options runtime \
            --sign "$SIGNING_IDENTITY" \
            "$app_bundle"
    fi

    echo "   ✅ Signed"
    echo ""
}

configure_dmg_finder_window() {
    local background_name="$1"
    local background_line=""

    if [[ -n "$background_name" ]]; then
        background_line="            set background picture to file \".background:${background_name}\""
    fi

    osascript <<EOF
tell application "Finder"
    tell disk "$APP_NAME"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set bounds of container window to {120, 140, 760, 540}

        tell icon view options of container window
            set arrangement to not arranged
            set icon size to 128
            set text size to 16
$background_line
        end tell

        set position of item "$APP_NAME.app" of container window to {180, 190}
        set position of item "Applications" of container window to {460, 190}

        update without registering applications
        close
        open
        delay 2
    end tell
end tell
EOF
}

# Step 5: Create DMG + notarize
step_dmg() {
    echo "── Step 5/5: Creating DMG ──"
    local app_bundle="$BUILD_DIR/$APP_NAME.app"
    local dmg_path="$BUILD_DIR/$DMG_NAME.dmg"
    local staging_dir="$BUILD_DIR/dmg-root"
    local temp_dmg="$BUILD_DIR/${DMG_NAME}-temp.dmg"
    local mount_dir=""
    local background_name=""
    local app_size_mb
    app_size_mb=$(du -sm "$app_bundle" | awk '{print $1}')
    local dmg_size_mb=$((app_size_mb + 128))

    cleanup_dmg() {
        if [[ -n "$mount_dir" ]]; then
            hdiutil detach "$mount_dir" >/dev/null 2>&1 || true
        fi
        rm -rf "$staging_dir"
        rm -f "$temp_dmg"
    }
    trap cleanup_dmg RETURN

    if [[ -n "$DMG_BACKGROUND_IMAGE" ]]; then
        if [[ ! -f "$DMG_BACKGROUND_IMAGE" ]]; then
            echo "Error: DMG background image not found at $DMG_BACKGROUND_IMAGE"
            exit 1
        fi
        background_name="$(basename "$DMG_BACKGROUND_IMAGE")"
    fi

    rm -f "$dmg_path" "$temp_dmg"
    rm -rf "$staging_dir"
    mkdir -p "$staging_dir"
    cp -R "$app_bundle" "$staging_dir/$APP_NAME.app"
    ln -s /Applications "$staging_dir/Applications"

    if [[ -n "$background_name" ]]; then
        mkdir -p "$staging_dir/.background"
        cp "$DMG_BACKGROUND_IMAGE" "$staging_dir/.background/$background_name"
    fi

    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$staging_dir" \
        -ov -format UDRW \
        -fs HFS+ \
        -size "${dmg_size_mb}m" \
        "$temp_dmg" >/dev/null

    mount_dir=$(hdiutil attach -readwrite -noverify -noautoopen "$temp_dmg" | awk -F '\t' '/Apple_HFS/ {print $3; exit}')
    if [[ -z "$mount_dir" ]]; then
        echo "Error: Failed to mount temporary DMG"
        exit 1
    fi

    if configure_dmg_finder_window "$background_name"; then
        if [[ -n "$background_name" ]]; then
            echo "   ✅ Finder layout + background image configured"
        else
            echo "   ✅ Finder layout configured"
        fi
    else
        echo "   ⚠ Finder layout customization failed; continuing with standard contents"
    fi

    bless --folder "$mount_dir" --openfolder "$mount_dir" >/dev/null 2>&1 || true
    sync
    hdiutil detach "$mount_dir" >/dev/null
    mount_dir=""

    hdiutil convert "$temp_dmg" \
        -ov -format UDZO \
        -imagekey zlib-level=9 \
        -o "$dmg_path" >/dev/null

    echo "   ✅ DMG: $dmg_path"
    if [[ -z "$background_name" ]]; then
        echo "   ℹ No DMG background image configured"
    fi

    if $RELEASE && [[ -n "$SIGNING_IDENTITY" ]]; then
        codesign --sign "$SIGNING_IDENTITY" "$dmg_path"
        echo "   ✅ DMG signed"

        if ! $SKIP_NOTARIZE; then
            echo "   Submitting for notarization..."
            xcrun notarytool submit "$dmg_path" \
                --keychain-profile "$NOTARIZE_PROFILE" \
                --wait
            xcrun stapler staple "$dmg_path"
            echo "   ✅ Notarized + stapled"
        fi
    fi

    if sparkle_updates_configured; then
        sign_dmg_for_sparkle "$dmg_path"
        generate_sparkle_appcast "$BUILD_DIR"
    else
        echo "   ℹ Sparkle signing skipped until SUFeedURL and SUPublicEDKey are fully configured"
    fi

    echo ""
    echo "🦊 Build complete!"
    echo "   DMG: $dmg_path"
    local size
    size=$(du -h "$dmg_path" | cut -f1)
    echo "   Size: $size"
}

# Run pipeline
step_engine
step_swift
step_assemble
step_sign
step_dmg
