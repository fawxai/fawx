# Fawx Release Runbook

Quick reference for builds, releases, testing, and common fixes.

---

## Release Build (Full Pipeline)

```bash
# 1. Promote dev → staging (PR required, branch is protected)
cd ~/fawx
gh pr create --base staging --head dev --title "Release vX.Y.Z" --body "Release notes here."
# Merge the PR on GitHub

# 2. Build + sign + notarize DMG from staging
git checkout staging && git pull origin staging
./scripts/build-dmg.sh --release --identity "Developer ID Application: Tkrm Ltd. (K8HJF9QW7N)"
# Output: build/Fawx.dmg (signed, notarized, stapled)

# 3. Promote staging → main (PR required)
gh pr create --base main --head staging --title "Release vX.Y.Z" --body "Release notes here."
# Merge the PR on GitHub

# 4. Tag the release
git checkout main && git pull origin main
git tag vX.Y.Z
git push origin vX.Y.Z

# 5. Upload build/Fawx.dmg to fawx.ai download page
```

## Debug Build (No Signing)

```bash
cd ~/fawx
./scripts/build-dmg.sh
# Output: build/Fawx.dmg (ad-hoc signed, not notarized)
```

## Skip Notarization (Signed but Faster)

```bash
./scripts/build-dmg.sh --release --skip-notarize --identity "Developer ID Application: Tkrm Ltd. (K8HJF9QW7N)"
```

---

## App Icon Update

```bash
# 1024x1024 PNG recommended
cp /path/to/new-icon.png ~/fawx/app/Fawx/Assets.xcassets/AppIcon.appiconset/icon.png

# Rebuild DMG after updating
./scripts/build-dmg.sh --release --identity "Developer ID Application: Tkrm Ltd. (K8HJF9QW7N)"
```

---

## VM Testing

### Clean Teardown (Full Reset)

```bash
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/ai.fawx.server.plist 2>/dev/null
rm -rf ~/.fawx ~/Library/LaunchAgents/ai.fawx.server.plist ~/Library/Logs/Fawx /Applications/Fawx.app
defaults delete ai.fawx.app 2>/dev/null
defaults delete ai.fawx.app.mac 2>/dev/null
security delete-generic-password -s "ai.fawx.app" 2>/dev/null
```

### Install DMG on VM

```bash
# Mount and copy (or just drag Fawx.app to /Applications in Finder)
hdiutil attach build/Fawx.dmg
cp -R /Volumes/Fawx/Fawx.app /Applications/
hdiutil detach /Volumes/Fawx
```

### Verify Fresh Install

1. Open Fawx.app
2. Welcome → Skip Tailscale → Provider step (should stay here)
3. Add API key or skip → Ready → Finish
4. Main session view loads
5. Check server is running: `ps aux | grep fawx-server`

---

## Troubleshooting

### Server Crash-Loop (Old Plist)

**Symptom:** `error: unexpected argument '--data-dir' found` in logs

```bash
# Check logs
tail -50 ~/Library/Logs/Fawx/server.log

# Fix: regenerate plist
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/ai.fawx.server.plist 2>/dev/null
/Applications/Fawx.app/Contents/MacOS/fawx-server bootstrap
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/ai.fawx.server.plist
```

### Stuck DMG Build (hdiutil Resource Busy)

```bash
# Find the stuck disk
hdiutil info | grep image-path

# Force eject
hdiutil detach /dev/diskN -force

# Re-run the DMG step
./scripts/build-dmg.sh --release --identity "Developer ID Application: Tkrm Ltd. (K8HJF9QW7N)"
```

### Wizard Skips Provider Step

**Symptom:** Setup wizard jumps from Tailscale straight to main session.

**Cause:** Stale `setup_complete` in UserDefaults from previous install.

```bash
defaults delete ai.fawx.app.mac 2>/dev/null
defaults delete ai.fawx.app 2>/dev/null
```

Then relaunch Fawx.

### Xcode Build Errors After Pull

```bash
# Clear derived data
rm -rf ~/Library/Developer/Xcode/DerivedData/Fawx-*

# Rebuild
cd ~/fawx/app
xcodebuild -scheme Fawx-macOS -configuration Debug build
```

### Check Config Permissions

```bash
ls -la ~/.fawx/config.toml
# Should be: -rw------- (0600)
```

---

## Key Identifiers

| Item | Value |
|------|-------|
| Signing identity | `Developer ID Application: Tkrm Ltd. (K8HJF9QW7N)` |
| Team ID | `K8HJF9QW7N` |
| Notarize profile | `fawx-notarize` |
| Bundle ID (macOS) | `ai.fawx.app.mac` |
| Bundle ID (generic) | `ai.fawx.app` |
| LaunchAgent label | `ai.fawx.server` |
| LaunchAgent plist | `~/Library/LaunchAgents/ai.fawx.server.plist` |
| Server log | `~/Library/Logs/Fawx/server.log` |
| Config | `~/.fawx/config.toml` |
| Session DB | `~/.fawx/sessions.redb` |
| DMG output | `~/fawx/build/Fawx.dmg` |
| Port range | `8400`–`8410` |
| Health endpoint | `http://127.0.0.1:<port>/health` |

---

## fawx-site (Landing Page)

```bash
# Master is protected; push to a branch and PR
cd ~/fawx-site
git checkout -b fix/description
# make changes
git push origin fix/description
gh pr create --base master --head fix/description --title "Fix: description"
# Merge PR on GitHub; Vercel auto-deploys from master
```

---

*Last updated: 2026-03-20*
