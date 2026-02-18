# Android NDK Cross-Compilation (aarch64-linux-android)

Issue: #118

This guide walks you through setting up reproducible Rust cross-compilation for Android arm64 (`aarch64-linux-android`) and building a hello-world binary that can run on a rooted Pixel 10 (or any rooted arm64 Android device).

## 1) Host prerequisites

- Linux host (examples assume Ubuntu)
- Rust toolchain with `rustup`
- Android SDK + Android NDK installed (tested with modern NDK r26+ layout)
- `adb` installed and authorized with your phone
- Rooted Pixel 10 (or any rooted arm64 Android device) with USB debugging enabled

Recommended packages:

```bash
sudo apt-get update
sudo apt-get install -y unzip file adb
```

If needed, install Rust:

```bash
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
```

## 2) NDK/toolchain setup

Set your NDK path:

```bash
export ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/<version>"
```

NDK clang linkers live at:

```text
$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
```

For this repo, we target API level 33 by default (matches `.cargo/config.toml`):

- Linker binary: `aarch64-linux-android33-clang`

Make it available on `PATH`:

```bash
export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
```

## 3) Rust target setup

Install Android Rust target:

```bash
rustup target add aarch64-linux-android
```

## 4) Linker/env configuration

This repository contains target config in `.cargo/config.toml`:

```toml
[target.aarch64-linux-android]
linker = "aarch64-linux-android33-clang"
ar = "llvm-ar"
```

You can also override linker explicitly via env (used by the helper script):

```bash
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android33-clang"
```

## 5) Build commands and artifact paths

### Option A: one-command helper (recommended)

```bash
./scripts/build-android-hello.sh
```

Output artifact:

```text
target/aarch64-linux-android/release/android-hello
```

### Option B: manual build

```bash
rustup target add aarch64-linux-android
cargo build -p ct-cli --bin android-hello --target aarch64-linux-android --release
```

## 6) Push + run on rooted Pixel 10 / arm64 Android (adb)

Build first, then:

```bash
adb devices
adb root
adb remount
adb push target/aarch64-linux-android/release/android-hello /data/local/tmp/android-hello
adb shell chmod 0755 /data/local/tmp/android-hello
adb shell /data/local/tmp/android-hello
```

Expected output:

```text
hello from citros android (aarch64-linux-android)
```

### 6.5) Validation checklist

After running the binary, verify:

1. Exit code is zero:

   ```bash
   adb shell '/data/local/tmp/android-hello; echo $?'
   ```

2. Output exactly matches:

   ```text
   hello from citros android (aarch64-linux-android)
   ```

3. Artifact format on host confirms arm64 ELF:

   ```bash
   file target/aarch64-linux-android/release/android-hello
   ```

   Should include `ELF 64-bit` and `ARM aarch64`.

4. Runtime dependencies resolve on device:

   ```bash
   adb shell ldd /data/local/tmp/android-hello
   ```

Optional cleanup:

```bash
adb shell rm /data/local/tmp/android-hello
```

## 7) Validation notes for this repo

- Build target: `aarch64-linux-android`
- Hello binary source: `crates/ct-cli/src/bin/android_hello.rs`
- Helper script: `scripts/build-android-hello.sh`

## 8) Troubleshooting

### `linker ... not found`

Your NDK toolchain bin directory is not on `PATH`, or `ANDROID_NDK_HOME` is wrong.

Check:

```bash
ls "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android33-clang"
```

### `target may not be installed`

Install it:

```bash
rustup target add aarch64-linux-android
```

### `adb root` fails

Device build may not allow adbd root. For rooted phones, use `adb shell su -c ...` pattern:

```bash
adb push target/aarch64-linux-android/release/android-hello /data/local/tmp/android-hello
adb shell su -c 'chmod 0755 /data/local/tmp/android-hello && /data/local/tmp/android-hello'
```

### `Permission denied` when executing binary

Ensure executable bit is set and path is executable:

```bash
adb shell chmod 0755 /data/local/tmp/android-hello
```

### API level mismatch errors

If your environment needs a different API level, update linker selection (e.g. `...android34-clang`) consistently in:

- `.cargo/config.toml`
- `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`
- script `ANDROID_API` env (`ANDROID_API=34 ./scripts/build-android-hello.sh`)
