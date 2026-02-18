#!/usr/bin/env bash
set -euo pipefail

# Reproducible helper for issue #118:
# Build ct-cli's android-hello binary for aarch64-linux-android.

if [[ "${1:-}" == "--help" ]] || [[ "${1:-}" == "-h" ]]; then
  cat <<EOF
Usage: $0

Builds ct-cli's android-hello binary for aarch64-linux-android.

Environment variables:
  ANDROID_NDK_HOME   Path to Android NDK (required)
  ANDROID_API        Android API level for clang linker suffix (default: 33)

Example:
  ANDROID_NDK_HOME=\"$HOME/Android/Sdk/ndk/27.0.12077973\" ANDROID_API=34 $0
EOF
  exit 0
fi

TARGET="aarch64-linux-android"
ANDROID_API="${ANDROID_API:-33}"

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  echo "error: ANDROID_NDK_HOME is not set" >&2
  echo "hint: export ANDROID_NDK_HOME=\"$HOME/Android/Sdk/ndk/<version>\"" >&2
  exit 1
fi

HOST_OS="$(uname -s)"
case "$HOST_OS" in
  Linux)
    PREBUILT_DIR="linux-x86_64"
    ;;
  Darwin)
    PREBUILT_DIR="darwin-x86_64"
    ;;
  *)
    echo "error: unsupported host OS: $HOST_OS" >&2
    echo "hint: use Linux or macOS host for this helper" >&2
    exit 1
    ;;
esac

TOOLCHAIN_BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$PREBUILT_DIR/bin"
LINKER="$TOOLCHAIN_BIN/aarch64-linux-android${ANDROID_API}-clang"

if [[ ! -x "$LINKER" ]]; then
  echo "error: linker not found: $LINKER" >&2
  echo "hint: verify ANDROID_API (${ANDROID_API}) and NDK installation" >&2
  exit 1
fi

export PATH="$TOOLCHAIN_BIN:$PATH"
export CC_aarch64_linux_android="$LINKER"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER"

rustup target add "$TARGET"

cargo build -p ct-cli --bin android-hello --target "$TARGET" --release

echo ""
echo "Built: target/$TARGET/release/android-hello"
file "target/$TARGET/release/android-hello" 2>&1 || echo "warning: 'file' command unavailable; install it to inspect ELF metadata"
