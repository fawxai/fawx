# Android NDK Cross-Compilation Setup

This guide explains how to set up cross-compilation for Android targets (required for Horizon 1).

## Prerequisites

- **Rust toolchain** with Android target support
- **Android NDK** r26 or later
- **Environment variables** configured

## Installation Steps

### 1. Install Android NDK

**Option A: Via Android Studio**
- Install Android Studio
- Open SDK Manager (Tools → SDK Manager)
- Navigate to SDK Tools tab
- Check "NDK (Side by side)" and "CMake"
- Click Apply to install

**Option B: Via Command Line (Linux/macOS)**
```bash
# Download NDK r26 (example)
wget https://dl.google.com/android/repository/android-ndk-r26-linux.zip
unzip android-ndk-r26-linux.zip -d ~/android-ndk
```

### 2. Set Environment Variables

Add to your `~/.bashrc` or `~/.zshrc`:

```bash
# Android NDK
export ANDROID_NDK_HOME="$HOME/android-ndk/android-ndk-r26"  # Adjust path as needed
export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"

# Standalone toolchain (if using)
export CC_aarch64_linux_android="aarch64-linux-android33-clang"
export AR_aarch64_linux_android="llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="aarch64-linux-android33-clang"
```

**Note:** Replace `android33` with your target API level (33 = Android 13).

Reload your shell:
```bash
source ~/.bashrc  # or source ~/.zshrc
```

### 3. Add Rust Android Target

```bash
rustup target add aarch64-linux-android
```

### 4. Verify Installation

```bash
# Check NDK tools are in PATH
which aarch64-linux-android-clang
# Should output: /path/to/ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android-clang

# Check Rust target is installed
rustup target list --installed | grep android
# Should show: aarch64-linux-android
```

## Building for Android

### Using Just (Recommended)

```bash
just check-android   # Check compilation without building
just build-android   # Full build (to be added)
```

### Manual Cargo Commands

```bash
# Check (fast, no binary output)
cargo check --target aarch64-linux-android

# Build
cargo build --target aarch64-linux-android --release

# Test (requires Android device/emulator)
cargo test --target aarch64-linux-android
```

## Troubleshooting

### "linker not found"
- Ensure `ANDROID_NDK_HOME` is set correctly
- Verify `aarch64-linux-android-clang` is in your `PATH`
- Check `.cargo/config.toml` linker configuration

### "error: failed to run custom build command"
- Some C dependencies may require additional NDK configuration
- Check crate-specific build instructions
- May need to set `BINDGEN_EXTRA_CLANG_ARGS` for bindgen crates

### API Level Mismatch
- Nova targets API 33 (Android 13) minimum
- Adjust `CC_aarch64_linux_android` to match your NDK version:
  - NDK r26 → `aarch64-linux-android33-clang`
  - NDK r25 → `aarch64-linux-android32-clang`

## Next Steps

- **Horizon 1:** Enable on-device LLM inference via `llama-cpp-sys`
- **Testing:** Deploy to Android device via `adb` or termux
- **CI/CD:** Automate Android builds in GitHub Actions

## References

- [Android NDK Downloads](https://developer.android.com/ndk/downloads)
- [Rust Android Targets](https://doc.rust-lang.org/nightly/rustc/platform-support/android.html)
- [cargo-ndk Tool](https://github.com/bbqsrc/cargo-ndk) (alternative build tool)
