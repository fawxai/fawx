# Android Root Setup (Magisk) for Fawx

> Issue: [#131](https://github.com/abbudjoe/fawx/issues/131)  
> Audience: developers preparing a dedicated test device for Horizon 1

This guide documents a **repeatable, safety-first** process for preparing a rooted Android device (Pixel recommended) for Fawx development.

## Read This First (Safety + Risk)

Rooting is intentionally invasive. Before you begin:

- **Bootloader unlocking wipes the device** (factory reset).
- Rooting can break OTA updates, SafetyNet/Play Integrity checks, and banking/media apps.
- A mistake while flashing can soft-brick the device.
- You should root a **dedicated development device**, not your primary phone.
- Back up anything important before starting.

If any step is unclear, stop and verify before proceeding.

---

## 1) Host Machine Prerequisites

Install and verify Android platform tools:

```bash
adb version
fastboot --version
```

You should have:

- `adb` available from your shell
- `fastboot` available from your shell

Useful references:
- <https://developer.android.com/tools/releases/platform-tools>
- Device factory images: <https://developers.google.com/android/images>

---

## 2) Phone Prerequisites

On the Android device:

1. Enable **Developer options** (tap Build number 7 times)
2. Enable **OEM unlocking**
3. Enable **USB debugging**
4. Connect device by USB and authorize your host key

Validate connection:

```bash
adb devices
```

Expected: your device serial appears as `device` (not `unauthorized`).

---

## 3) Unlock Bootloader

Reboot into bootloader:

```bash
adb reboot bootloader
```

Confirm fastboot sees the device:

```bash
fastboot devices
```

Unlock bootloader (exact command varies by device generation):

```bash
fastboot flashing unlock
# or on older devices:
# fastboot oem unlock
```

Confirm on-device prompt. Device will wipe/reset.

After reboot and initial setup, re-enable Developer options + USB debugging.

---

## 4) Patch Boot Image with Magisk

1. Download the exact factory image matching your current build.
2. Extract `boot.img` from the factory image package.
3. Install Magisk app on device.
4. In Magisk: **Install → Select and Patch a File** → choose `boot.img`.
5. Pull patched image back to host:

```bash
adb pull /sdcard/Download/magisk_patched*.img
```

Rename for clarity (optional):

```bash
mv magisk_patched*.img magisk_patched_boot.img
```

---

## 5) Flash Patched Boot Image

Reboot to bootloader:

```bash
adb reboot bootloader
```

Flash patched boot image:

```bash
fastboot flash boot magisk_patched_boot.img
```

Reboot:

```bash
fastboot reboot
```

---

## 6) Verify Root + Baseline System State

Open Magisk and verify status is installed, then run:

```bash
adb shell su -c id
```

Expected output includes `uid=0(root)`.

Optional checks relevant for Fawx bring-up:

```bash
adb shell getenforce  # Should output "Enforcing"
```

- Keep SELinux **Enforcing** unless you are explicitly testing permissive-mode behavior.
- If permissive mode is temporarily required for a specific experiment, document it and revert.

---

## 7) Magisk Hide / SafetyNet Compatibility

### Does Fawx require root?

**No.** Fawx uses Android's Accessibility Service API for screen reading and phone control — this is a standard, non-root Android API. Root is only needed for the optional Rust daemon (`ct-cli`) component.

### SafetyNet / Play Integrity

Since Fawx's core phone control runs through Accessibility Service (not root), Magisk Hide / Zygisk DenyList should not affect Fawx functionality. Banking apps and other SafetyNet-dependent apps can coexist with Fawx on the same device.

### Edge cases with Magisk

If you **are** running Fawx on a rooted device with Magisk:

- **Do NOT add Fawx to the Magisk DenyList** — this would prevent the optional Rust daemon from accessing root if needed
- **Magisk modules that modify the accessibility framework** (e.g., Xposed modules targeting `AccessibilityService`) may interfere with Fawx — disable them if you encounter issues
- **SELinux policy modules** — keep SELinux in Enforcing mode. If a Magisk module switches to Permissive, revert it. Fawx doesn't need Permissive mode.
- **App cloning / dual-space modules** — Fawx is not tested in cloned app environments and may not function correctly

### Recommended Magisk configuration for Fawx development

1. Keep Magisk DenyList **enabled** for banking/payment apps only
2. Do **not** add Fawx to the DenyList
3. Avoid broad SELinux policy modules during testing
4. If issues arise, test with all Magisk modules disabled to isolate the cause

---

## 8) Development Hygiene for Rooted Devices

Recommended guardrails for rooted test devices:

- Use this device only for dev/test workloads.
- Keep a copy of stock `boot.img` to roll back quickly.
- Document every root-affecting change (Magisk modules, policy tweaks).
- Avoid broad/unknown Magisk modules during core platform bring-up.

Rollback to stock boot (if needed):

```bash
adb reboot bootloader
fastboot flash boot boot.img
fastboot reboot
```

(Use the stock `boot.img` from the matching factory image.)

---

## 9) Fawx Next Steps After Root

Once rooted, continue with:

1. `docs/android-setup.md` for Rust + NDK cross-compilation setup
2. `docs/android-adb-workflow.md` for day-to-day deploy/debug workflow

This keeps root provisioning separate from iterative development loops.
