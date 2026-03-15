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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
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

# Parse args
while [[ $# -gt 0 ]]; do
    case "$1" in
        --release) RELEASE=true; shift ;;
        --skip-notarize) SKIP_NOTARIZE=true; shift ;;
        --identity) SIGNING_IDENTITY="$2"; shift 2 ;;
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

# Step 5: Create DMG + notarize
step_dmg() {
    echo "── Step 5/5: Creating DMG ──"
    local app_bundle="$BUILD_DIR/$APP_NAME.app"
    local dmg_path="$BUILD_DIR/$DMG_NAME.dmg"

    rm -f "$dmg_path"
    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$app_bundle" \
        -ov -format UDZO \
        "$dmg_path"

    echo "   ✅ DMG: $dmg_path"

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
