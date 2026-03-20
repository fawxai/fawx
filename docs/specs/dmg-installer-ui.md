# Spec: DMG Installer with Drag-to-Applications UI

## Problem

The DMG currently contains only `Fawx.app` with no visual guidance. Users open
the app directly from the mounted DMG volume instead of copying to Applications.
This causes:
- LaunchAgent plist points to `/Volumes/Fawx/...` which disappears when DMG is ejected
- Server crash-loops after DMG unmount
- Confusing failure mode for new users

## Fix

Update `scripts/build-dmg.sh` step 5 (`step_dmg`) to create a proper macOS
installer DMG with:

1. An `/Applications` symlink inside the DMG
2. A background image showing a drag arrow from the app icon to the Applications folder
3. Icon positioning so the app and Applications folder are side by side

### Implementation

Replace the simple `hdiutil create -srcfolder` with a staging approach:

```bash
step_dmg() {
    echo "── Step 5/5: Creating DMG ──"
    local app_bundle="$BUILD_DIR/$APP_NAME.app"
    local dmg_path="$BUILD_DIR/$DMG_NAME.dmg"
    local staging_dir="$BUILD_DIR/dmg-staging"

    # Create staging directory with app + Applications symlink
    rm -rf "$staging_dir"
    mkdir -p "$staging_dir"
    cp -R "$app_bundle" "$staging_dir/"
    ln -s /Applications "$staging_dir/Applications"

    # Copy background image if it exists
    if [[ -f "$REPO_ROOT/assets/dmg-background.png" ]]; then
        mkdir -p "$staging_dir/.background"
        cp "$REPO_ROOT/assets/dmg-background.png" "$staging_dir/.background/background.png"
    fi

    rm -f "$dmg_path"

    # Create DMG with custom window settings via create-dmg (if available)
    # or fall back to plain hdiutil
    if command -v create-dmg &>/dev/null; then
        local create_dmg_args=(
            --volname "$APP_NAME"
            --window-pos 200 120
            --window-size 660 400
            --icon-size 80
            --icon "$APP_NAME.app" 180 200
            --app-drop-link 480 200
            --no-internet-enable
        )
        if [[ -f "$REPO_ROOT/assets/dmg-background.png" ]]; then
            create_dmg_args+=(--background "$REPO_ROOT/assets/dmg-background.png")
        fi
        create-dmg "${create_dmg_args[@]}" "$dmg_path" "$staging_dir"
    else
        # Fallback: plain hdiutil (no icon positioning, but has Applications symlink)
        hdiutil create -volname "$APP_NAME" \
            -srcfolder "$staging_dir" \
            -ov -format UDZO \
            "$dmg_path"
    fi

    echo "   ✅ DMG: $dmg_path"
    rm -rf "$staging_dir"

    # ... existing signing + notarization code unchanged
}
```

### Background Image

Create `assets/dmg-background.png`:
- 660x400 pixels
- Dark background matching Fawx brand
- Subtle arrow or visual cue pointing from left (app) to right (Applications)
- Keep it minimal — the icon positioning does most of the work

### create-dmg Installation

```bash
brew install create-dmg
```

If `create-dmg` is not installed, the script falls back to plain `hdiutil`
which still includes the Applications symlink (just without icon positioning).

## Files to Modify

1. `scripts/build-dmg.sh` — replace `step_dmg()` function
2. **Create** `assets/dmg-background.png` — installer background image (optional)

## Testing

1. Run `./scripts/build-dmg.sh`
2. Mount the DMG
3. Verify: app icon on left, Applications shortcut on right
4. Drag app to Applications, eject DMG
5. Launch from Applications — server should start correctly
