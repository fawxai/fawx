# Android Troubleshooting FAQ

> Issue: [#253](https://github.com/abbudjoe/citros/issues/253)  
> Audience: developers and testers setting up Citros on Android

Common issues and solutions for Citros Android development and testing.

**Package name:** The installable Citros APK uses `ai.citros.app`. The `:chat` library module uses namespace `ai.citros.chat`.

## Quick Reference

- [ADB Connection Issues](#adb-connection-issues)
- [Accessibility Service Issues](#accessibility-service-issues)
- [APK Installation Issues](#apk-installation-issues)
- [API Key and Provider Issues](#api-key-and-provider-issues)
- [Overlay Permission Issues](#overlay-permission-issues)
- [Build Issues](#build-issues)
- [Logcat Filtering](#logcat-filtering)

---

## ADB Connection Issues

### Device not showing in `adb devices`

**Symptoms:** No device listed, or device shows as `offline`.

**Solutions:**
1. Check USB cable — use a data cable, not charge-only
2. Try a different USB port (prefer rear motherboard ports)
3. On the phone: Settings → Developer options → revoke USB debugging authorizations, then re-authorize
4. Restart ADB server:
   ```bash
   adb kill-server
   adb start-server
   adb devices
   ```
5. On Linux, check udev rules for your device vendor ID

### Device shows as `unauthorized`

**Cause:** Host key not yet accepted on the device.

**Fix:** Unlock the phone screen — you should see an "Allow USB debugging?" prompt. Check "Always allow from this computer" and tap OK.

### Wireless ADB disconnects frequently

**Cause:** Network instability or device sleeping.

**Fix:**
- Keep device on the same Wi-Fi network as the host
- Disable battery optimization for developer tools
- Re-connect with `adb connect <ip>:5555` when dropped
- For reliable iteration, prefer wired USB

---

## Accessibility Service Issues

### Citros Accessibility Service not showing in Settings

**Symptoms:** The "Citros Phone Control" option doesn't appear under Settings → Accessibility → Downloaded services.

**Solutions:**
1. Ensure the app is fully installed — not just side-loaded as an APK but launched at least once
2. Clear the Accessibility Service cache:
   ```bash
   adb shell settings put secure enabled_accessibility_services ""
   ```
3. Reboot the device:
   ```bash
   adb reboot
   ```
4. Check if the service is declared in the manifest:
   ```bash
   adb shell dumpsys package ai.citros.app | grep -A5 "accessibility"
   ```

### Accessibility Service keeps disabling itself

**Cause:** Android's battery optimization or "restricted app" settings may kill the service.

**Fix:**
1. Settings → Battery → Unrestricted for Citros
2. Settings → Apps → Citros → Battery → Unrestricted
3. On some OEMs (Samsung, Xiaomi), also disable "adaptive battery" for Citros

---

## APK Installation Issues

### `INSTALL_FAILED_VERSION_DOWNGRADE`

**Cause:** Trying to install an older version over a newer one.

**Fix:**
```bash
adb uninstall ai.citros.app
# Then reinstall
cd android && ./gradlew :app:installDebug
```

### `INSTALL_FAILED_TEST_ONLY`

**Cause:** Debug APK on a non-debuggable build.

**Fix:**
```bash
adb install -t path/to/app-debug.apk
```

### `INSTALL_FAILED_INSUFFICIENT_STORAGE`

**Fix:** Free space on the device or clear app caches:
```bash
adb shell pm clear ai.citros.app
```

### APK installs but app crashes on launch

**Debug steps:**
1. Check logs immediately after launch:
   ```bash
   adb logcat -c && adb logcat -s AndroidRuntime:E
   ```
2. Common causes:
   - Missing native library (`UnsatisfiedLinkError`) — rebuild with correct NDK target
   - Missing permission — check manifest declarations
   - ProGuard stripping needed classes — check R8 rules

---

## API Key and Provider Issues

### "Could not detect provider" error

**Cause:** The API key format isn't recognized by Citros's auto-detection.

**Fix:**
- Anthropic keys start with `sk-ant-api03-`
- OpenAI keys start with `sk-` (but not `sk-ant-`)
- OpenRouter keys start with `sk-or-`
- If auto-detection fails, select the provider explicitly in Settings → API Keys

### "Authentication failed" or 401 errors

**Solutions:**
1. Verify the key is valid and not expired at the provider's dashboard
2. Check for leading/trailing whitespace in the key
3. Ensure you have billing set up / credits available on the provider account
4. Try the key with curl:
   ```bash
   # Anthropic (replace MODEL_ID with your configured model,
   # e.g., claude-sonnet-4-20250514 or claude-haiku-4-20250514)
   curl https://api.anthropic.com/v1/messages \
     -H "x-api-key: YOUR_KEY" \
     -H "anthropic-version: 2023-06-01" \
     -H "content-type: application/json" \
     -d '{"model":"MODEL_ID","max_tokens":10,"messages":[{"role":"user","content":"ping"}]}'
   # Replace MODEL_ID with your model, e.g., claude-sonnet-4-20250514
   ```

### Model ID format errors

**Cause:** Incorrect model identifier format for the provider.

**Expected formats:**
- **Anthropic:** `claude-sonnet-4-20250514`, `claude-haiku-4-20250514`
- **OpenAI:** `gpt-4o`, `gpt-4o-mini`
- **OpenRouter:** `anthropic/claude-sonnet-4-20250514`, `openai/gpt-4o`

Model IDs are configured in Settings → Models. The default models are set per-provider and should work out of the box.

---

## Overlay Permission Issues

### Overlay doesn't appear during phone control

**Cause:** `SYSTEM_ALERT_WINDOW` permission not granted.

**Fix:**
1. Settings → Apps → Citros → Display over other apps → Allow
2. Or via ADB:
   ```bash
   adb shell appops set ai.citros.app SYSTEM_ALERT_WINDOW allow
   ```

### Overlay appears but is blank/black

**Cause:** Hardware acceleration issue on some devices.

**Fix:** Try restarting the app. If persistent, report device model and Android version.

---

## Build Issues

### `JAVA_HOME is not set`

**Fix:** Install JDK 17+ and set the environment variable:
```bash
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk
```

### Gradle build fails with "SDK not found"

**Fix:** Create or update `android/local.properties`:
```
sdk.dir=/path/to/your/Android/Sdk
```

### Tests fail with `UninitializedPropertyAccessException`

**Cause:** Android-dependent tests running without Robolectric.

**Fix:** Ensure test classes that use Android APIs have `@RunWith(RobolectricTestRunner::class)`.

---

## Logcat Filtering

For efficient debugging, filter logcat to Citros-specific tags:

```bash
adb logcat -s CitrosAccessibility:* ChatViewModel:* PhoneAgentApi:* OverlayService:* SqliteMemoryProvider:*
```

See also:
- [ADB Development Workflow](android-adb-workflow.md) for the full debug loop
- [Android Root Setup (Magisk)](android-root-magisk-setup.md) for rooted device configuration
- [Wallet & Storage Paths](wallet-storage-paths.md) for credential storage details
