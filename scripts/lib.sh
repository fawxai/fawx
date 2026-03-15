#!/usr/bin/env bash

# Shared shell helpers for repo build scripts.

detect_cpu_count() {
  if command -v getconf >/dev/null 2>&1; then
    getconf _NPROCESSORS_ONLN 2>/dev/null && return
  fi
  if command -v sysctl >/dev/null 2>&1; then
    sysctl -n hw.ncpu 2>/dev/null && return
  fi
  echo 1
}

resolve_tool() {
  local tool="$1"
  if command -v "$tool" >/dev/null 2>&1; then
    command -v "$tool"
    return
  fi
  if [[ -x "$HOME/.cargo/bin/$tool" ]]; then
    echo "$HOME/.cargo/bin/$tool"
    return
  fi
  echo "$tool not found in PATH or ~/.cargo/bin" >&2
  return 1
}
