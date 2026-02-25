#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_DIR="$ROOT_DIR/android"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/test-atomic.sh <bucket> [--iterations N]

Buckets:
  p0.chat-lint            Run :chat:lint
  p0.core-sensor-ci       Run :core:phoneAgentApiSensorCiTest
  p0.chat-sensor-ci       Run :chat:androidSensorProviderCiTest
  p0                      Run all P0 buckets

  p1.core-unit            Run :core:testDebugUnitTest (flaky disabled)
  p1.chat-unit            Run :chat:testDebugUnitTest (flaky disabled)
  p1                      Run all P1 buckets

  p2.soak-sensor-repeat   Repeat P0 sensor buckets (default iterations: 5)
  p2.flaky-audit          Run core/chat unit tests with flaky enabled
  p2                      Run all P2 buckets
EOF
}

validate_environment() {
  if [[ "${TEST_ATOMIC_ENV_VALIDATED:-0}" == "1" ]]; then
    return
  fi

  if [[ ! -x "$ANDROID_DIR/gradlew" ]]; then
    echo "Android Gradle wrapper is missing or not executable: $ANDROID_DIR/gradlew" >&2
    exit 1
  fi

  if [[ -z "${JAVA_HOME:-}" ]]; then
    echo "JAVA_HOME is not set. Export JAVA_HOME to a JDK path before running atomic buckets." >&2
    exit 1
  fi

  if [[ ! -d "$JAVA_HOME" ]]; then
    echo "JAVA_HOME does not point to an existing directory: $JAVA_HOME" >&2
    exit 1
  fi

  if [[ -z "${ANDROID_SDK_ROOT:-}" ]]; then
    if [[ -n "${ANDROID_HOME:-}" ]]; then
      export ANDROID_SDK_ROOT="$ANDROID_HOME"
      echo "ANDROID_SDK_ROOT is not set; using ANDROID_HOME=$ANDROID_HOME" >&2
    else
      echo "ANDROID_SDK_ROOT is not set (ANDROID_HOME fallback is also unset)." >&2
      echo "Export ANDROID_SDK_ROOT before running atomic buckets." >&2
      exit 1
    fi
  fi

  if [[ ! -d "$ANDROID_SDK_ROOT" ]]; then
    echo "ANDROID_SDK_ROOT does not point to an existing directory: $ANDROID_SDK_ROOT" >&2
    exit 1
  fi

  export TEST_ATOMIC_ENV_VALIDATED=1
}

run_gradle() {
  validate_environment

  local ci_args=()
  if [[ "${CI:-}" == "true" || "${GITHUB_ACTIONS:-}" == "true" ]]; then
    ci_args+=(--no-daemon)
  fi

  (cd "$ANDROID_DIR" && ./gradlew "${ci_args[@]}" "$@")
}

run_bucket_group() {
  local group_name="$1"
  shift

  local failed=0
  local sub_bucket
  for sub_bucket in "$@"; do
    echo "[$group_name] running $sub_bucket"
    if ! "$0" "$sub_bucket"; then
      echo "[$group_name] FAILED: $sub_bucket" >&2
      failed=1
    else
      echo "[$group_name] PASSED: $sub_bucket"
    fi
  done

  if [[ "$failed" -ne 0 ]]; then
    echo "[$group_name] one or more sub-buckets failed" >&2
    exit 1
  fi
}

run_p2_group() {
  local failed=0

  echo "[p2] running p2.soak-sensor-repeat --iterations $iterations"
  if ! "$0" p2.soak-sensor-repeat --iterations "$iterations"; then
    echo "[p2] FAILED: p2.soak-sensor-repeat" >&2
    failed=1
  else
    echo "[p2] PASSED: p2.soak-sensor-repeat"
  fi

  echo "[p2] running p2.flaky-audit"
  if ! "$0" p2.flaky-audit; then
    echo "[p2] FAILED: p2.flaky-audit" >&2
    failed=1
  else
    echo "[p2] PASSED: p2.flaky-audit"
  fi

  if [[ "$failed" -ne 0 ]]; then
    echo "[p2] one or more sub-buckets failed" >&2
    exit 1
  fi
}

iterations=5
bucket="${1:-}"

if [[ -z "$bucket" ]]; then
  usage
  exit 1
fi

if [[ "$bucket" == "-h" || "$bucket" == "--help" ]]; then
  usage
  exit 0
fi

shift || true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iterations)
      if [[ $# -lt 2 ]]; then
        echo "--iterations requires a value" >&2
        usage
        exit 1
      fi
      iterations="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown arg: $1" >&2
      usage
      exit 1
      ;;
  esac
done

case "$bucket" in
  p0.chat-lint)
    run_gradle :chat:lint
    ;;
  p0.core-sensor-ci)
    run_gradle -PcitrosRunFlakyTests=false :core:phoneAgentApiSensorCiTest
    ;;
  p0.chat-sensor-ci)
    run_gradle -PcitrosRunFlakyTests=false :chat:androidSensorProviderCiTest
    ;;
  p0)
    run_bucket_group "p0" p0.chat-lint p0.core-sensor-ci p0.chat-sensor-ci
    ;;

  p1.core-unit)
    run_gradle -PcitrosRunFlakyTests=false :core:testDebugUnitTest
    ;;
  p1.chat-unit)
    run_gradle -PcitrosRunFlakyTests=false :chat:testDebugUnitTest
    ;;
  p1)
    run_bucket_group "p1" p1.core-unit p1.chat-unit
    ;;

  p2.soak-sensor-repeat)
    if ! [[ "$iterations" =~ ^[0-9]+$ ]] || [[ "$iterations" -lt 1 ]]; then
      echo "--iterations must be a positive integer" >&2
      exit 1
    fi

    for ((i=1; i<=iterations; i++)); do
      echo "[p2.soak-sensor-repeat] iteration $i/$iterations"
      run_gradle --continue -PcitrosRunFlakyTests=false :core:phoneAgentApiSensorCiTest :chat:androidSensorProviderCiTest
    done
    ;;
  p2.flaky-audit)
    run_gradle --continue -PcitrosRunFlakyTests=true :core:testDebugUnitTest :chat:testDebugUnitTest
    ;;
  p2)
    run_p2_group
    ;;

  *)
    echo "Unknown bucket: $bucket" >&2
    usage
    exit 1
    ;;
esac
