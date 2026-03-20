#!/usr/bin/env bash
# Copy to build-dmg-config.sh and fill in your values
export FAWX_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export FAWX_NOTARIZE_PROFILE="fawx-notarize"
# Optional: custom Finder background for the DMG installer window.
# export FAWX_DMG_BACKGROUND_IMAGE="/absolute/path/to/dmg-background.png"
# Set up notarize profile:
# xcrun notarytool store-credentials fawx-notarize --apple-id you@email.com --team-id TEAMID
