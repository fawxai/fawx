# Spec: Sparkle Auto-Update Integration

## Problem
Fawx distributes updates as notarized DMGs downloaded from fawx.ai. Users must manually check the website, download, and replace the app. With external users starting to adopt Fawx, we need automatic update delivery.

## Solution
Integrate the Sparkle 2 framework (SPM) for automatic update checking and in-app update prompts. Users get notified of new versions and can update with one click.

## Scope
- macOS only (Sparkle does not support iOS)
- SwiftUI integration (no XIB/storyboard)
- EdDSA signing for update verification
- Appcast hosted on fawx.ai
- "Check for Updates" menu item
- Automatic background update checks on launch

## Implementation

### 1. Add Sparkle SPM Dependency

In `Fawx.xcodeproj`:
- File > Add Packages > `https://github.com/sparkle-project/Sparkle`
- Link the `Sparkle` framework to the **macOS target only** (not iOS)
- Minimum Sparkle version: 2.5+

### 2. Generate EdDSA Signing Keys

After adding the SPM package, find the `generate_keys` tool at:
```
~/Library/Developer/Xcode/DerivedData/.../SourcePackages/artifacts/sparkle/Sparkle/bin/generate_keys
```

Or use the Sparkle release binary:
```bash
./bin/generate_keys
```

This outputs:
- A **private key** stored in the macOS Keychain (used for signing updates)
- A **public key** string to embed in Info.plist as `SUPublicEDKey`

**IMPORTANT:** The private key lives in the Keychain of the machine that signs releases. Never export or share it. The Mac Mini (release machine) should generate and hold this key.

### 3. Info.plist Changes (macOS only: `app/Fawx/Info.plist`)

Add:
```xml
<key>SUFeedURL</key>
<string>https://fawx.ai/appcast.xml</string>

<key>SUPublicEDKey</key>
<string>PASTE_PUBLIC_KEY_HERE</string>
```

Joe will paste the actual public key after running `generate_keys` on the Mac Mini.

Also ensure versioning is correct:
```xml
<key>CFBundleShortVersionString</key>
<string>1.1.0</string>

<key>CFBundleVersion</key>
<string>2</string>
```

`CFBundleVersion` must be an incrementing integer (Sparkle uses this for comparison). Bump it with every release. `CFBundleShortVersionString` is the human-readable version.

### 4. SwiftUI Integration

#### 4a. Create `SparkleUpdater.swift` (macOS only)

Create `app/Fawx/Services/SparkleUpdater.swift`:

```swift
#if os(macOS)
import Foundation
import Sparkle

/// Manages Sparkle auto-update lifecycle for macOS.
@MainActor
final class SparkleUpdater: ObservableObject {
    private let updaterController: SPUStandardUpdaterController

    @Published var canCheckForUpdates = false

    init() {
        updaterController = SPUStandardUpdaterController(
            startingUpdater: true,
            updaterDelegate: nil,
            userDriverDelegate: nil
        )

        updaterController.updater.publisher(for: \.canCheckForUpdates)
            .assign(to: &$canCheckForUpdates)
    }

    func checkForUpdates() {
        updaterController.updater.checkForUpdates()
    }
}
#endif
```

#### 4b. Wire into `FawxApp.swift`

Add the updater as a `@State` property (macOS only):

```swift
#if os(macOS)
@State private var sparkleUpdater = SparkleUpdater()
#endif
```

Pass it to the commands modifier:

```swift
.commands {
    FawxMacCommands(
        appState: appState,
        sessionViewModel: sessionViewModel,
        chatViewModel: chatViewModel,
        sparkleUpdater: sparkleUpdater  // new parameter
    )
}
```

#### 4c. Add "Check for Updates" menu item in `FawxMacCommands.swift`

Add a `sparkleUpdater` parameter to `FawxMacCommands`:

```swift
struct FawxMacCommands: Commands {
    @Bindable var appState: AppState
    @Bindable var sessionViewModel: SessionViewModel
    @Bindable var chatViewModel: ChatViewModel
    var sparkleUpdater: SparkleUpdater  // new

    var body: some Commands {
        // Existing command groups...

        CommandGroup(after: .appInfo) {
            Button("Check for Updates...") {
                sparkleUpdater.checkForUpdates()
            }
            .disabled(!sparkleUpdater.canCheckForUpdates)
        }

        // ... rest of existing commands
    }
}
```

This places "Check for Updates..." in the app menu right below "About Fawx".

### 5. Update Release Pipeline

#### 5a. Update `scripts/build-dmg.sh`

After building and notarizing the DMG, sign it for Sparkle:

```bash
# Sign for Sparkle (EdDSA) -- after notarization
SPARKLE_SIGN="$(find ~/Library/Developer/Xcode/DerivedData -path '*/artifacts/sparkle/Sparkle/bin/sign_update' -print -quit 2>/dev/null)"
if [ -n "$SPARKLE_SIGN" ]; then
    echo "Signing DMG for Sparkle..."
    $SPARKLE_SIGN "$DMG_PATH"
    # Output: sparkle:edSignature="..." length="..."
fi
```

#### 5b. Generate Appcast

After signing, generate/update the appcast:

```bash
GENERATE_APPCAST="$(find ~/Library/Developer/Xcode/DerivedData -path '*/artifacts/sparkle/Sparkle/bin/generate_appcast' -print -quit 2>/dev/null)"
if [ -n "$GENERATE_APPCAST" ]; then
    # Point at a directory containing the DMG(s)
    $GENERATE_APPCAST /path/to/dmg/directory/
    # Produces/updates appcast.xml in that directory
fi
```

#### 5c. Updated Release Runbook Steps

Add after step 2 (build DMG) in `docs/checklists/release-runbook.md`:

```
# 2b. Sign DMG for Sparkle auto-update
./bin/sign_update build/Fawx.dmg
# Note the edSignature and length output

# 2c. Generate/update appcast
./bin/generate_appcast build/
# Produces build/appcast.xml

# 5. Upload BOTH build/Fawx.dmg AND build/appcast.xml to fawx.ai
```

### 6. Appcast Hosting

Host `appcast.xml` at `https://fawx.ai/appcast.xml`. The `generate_appcast` tool produces this automatically from a directory of signed DMGs.

Example appcast structure:
```xml
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Fawx Updates</title>
    <link>https://fawx.ai</link>
    <item>
      <title>Version 1.2.0</title>
      <sparkle:version>3</sparkle:version>
      <sparkle:shortVersionString>1.2.0</sparkle:shortVersionString>
      <pubDate>Thu, 20 Mar 2026 14:00:00 +0000</pubDate>
      <enclosure
        url="https://fawx.ai/downloads/Fawx-1.2.0.dmg"
        sparkle:edSignature="BASE64_SIGNATURE_HERE"
        length="12345678"
        type="application/octet-stream" />
    </item>
  </channel>
</rss>
```

## Files Changed

| File | Change |
|------|--------|
| `Fawx.xcodeproj/project.pbxproj` | Add Sparkle SPM package reference (macOS target only) |
| `app/Fawx/Info.plist` | Add `SUFeedURL`, `SUPublicEDKey`, bump `CFBundleVersion` |
| `app/Fawx/Services/SparkleUpdater.swift` | **New** -- Sparkle wrapper class |
| `app/Fawx/FawxApp.swift` | Initialize and pass `SparkleUpdater` |
| `app/Fawx/Views/macOS/FawxMacCommands.swift` | Add "Check for Updates" menu item |
| `scripts/build-dmg.sh` | Add Sparkle signing step after notarization |
| `docs/checklists/release-runbook.md` | Add Sparkle signing + appcast steps |

## Manual Steps (Joe)

These cannot be automated by the implementer:

1. **Generate EdDSA keys on Mac Mini:** Run `generate_keys` from Sparkle tools. This stores the private key in Keychain and outputs the public key.
2. **Paste public key into Info.plist:** Replace `PASTE_PUBLIC_KEY_HERE` with the actual key.
3. **Set up appcast hosting:** Ensure `https://fawx.ai/appcast.xml` serves the generated file.
4. **Bump `CFBundleVersion`:** Set to `2` (or next integer) for the first Sparkle-enabled release.

## Testing

1. Build and run the app
2. Verify "Check for Updates..." appears in the Fawx menu (below "About Fawx")
3. Click it; Sparkle should attempt to fetch the appcast URL (will fail gracefully if not yet hosted)
4. Verify no Sparkle UI appears on iOS builds
5. Verify the app launches without errors (Sparkle auto-checks on startup)

## Notes

- Sparkle handles the entire download, verification, and restart flow automatically
- Delta updates (binary diffs between versions) are supported by `generate_appcast` for faster downloads
- Sparkle respects macOS notification preferences; users can snooze or skip versions
- The `SUPublicEDKey` is safe to commit (it is the public half; the private key stays in Keychain)
- iOS updates go through the App Store; Sparkle is macOS only
