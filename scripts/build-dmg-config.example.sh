#!/usr/bin/env bash
# Copy to build-dmg-config.sh and fill in your values
export FAWX_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export FAWX_NOTARIZE_PROFILE="fawx-notarize"
# Set up notarize profile:
# xcrun notarytool store-credentials fawx-notarize --apple-id you@email.com --team-id TEAMID
