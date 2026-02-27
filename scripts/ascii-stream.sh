#!/usr/bin/env bash
set -euo pipefail

# ascii-stream.sh
# Stream/rerender an image as ASCII in a terminal, adapting to terminal size and capabilities.
# Requires: ascii-image-converter, tput
# Optional: tmux/screen supported (best-effort), colors auto-detected.

usage() {
  cat <<'EOF'
Usage:
  ascii-stream.sh [options] <image>

Options:
  -m, --mode   auto|braille|ascii      (default: auto)
  -w, --watch                      Re-render on resize (and optionally on timer)
  -r, --rate   <seconds>           Re-render every N seconds in watch mode (default: 0 = only on resize)
  -W, --max-width <cols>           Clamp render width (default: 240)
  --no-color                       Disable color output
  --no-dither                      Disable dithering (braille mode only)
  -t, --threshold <0-255>          Braille threshold (default: 28; lower = more dots)
  --no-clear                       Don't clear screen before rendering

Examples:
  ./ascii-stream.sh fawx.png
  ./ascii-stream.sh -m braille --threshold 24 --watch fawx.png
  ./ascii-stream.sh -m ascii --no-color --watch fawx.png
EOF
}

MODE="auto"
WATCH=0
RATE=0
MAXW=240
NO_COLOR=0
DITHER=1
THRESH=28
CLEAR=1

# Parse args
IMG=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--mode) MODE="${2:-}"; shift 2;;
    -w|--watch) WATCH=1; shift;;
    -r|--rate) RATE="${2:-0}"; shift 2;;
    -W|--max-width) MAXW="${2:-240}"; shift 2;;
    --no-color) NO_COLOR=1; shift;;
    --no-dither) DITHER=0; shift;;
    -t|--threshold) THRESH="${2:-28}"; shift 2;;
    --no-clear) CLEAR=0; shift;;
    -h|--help) usage; exit 0;;
    -*)
      echo "Unknown option: $1" >&2
      usage; exit 2;;
    *)
      IMG="$1"; shift;;
  esac
done

if [[ -z "${IMG}" ]]; then
  usage; exit 2
fi

command -v ascii-image-converter >/dev/null 2>&1 || {
  echo "Error: ascii-image-converter not found in PATH." >&2
  echo "Install (brew): brew install ascii-image-converter" >&2
  exit 1
}
command -v tput >/dev/null 2>&1 || {
  echo "Error: tput not found (ncurses)." >&2
  exit 1
}

# Determine if we should enable color
supports_color() {
  [[ $NO_COLOR -eq 1 ]] && return 1
  [[ ! -t 1 ]] && return 1
  [[ "${TERM:-}" == "dumb" ]] && return 1
  local colors
  colors="$(tput colors 2>/dev/null || echo 0)"
  [[ "${colors}" -ge 8 ]]
}

# Heuristic: choose mode in "auto"
auto_mode() {
  # Braille needs UTF-8 locale and enough columns to look worthwhile
  local cols
  cols="$(tput cols 2>/dev/null || echo 80)"
  if [[ "${LC_CTYPE:-}${LANG:-}" == *"UTF-8"* ]] && [[ "$cols" -ge 90 ]]; then
    echo "braille"
  else
    echo "ascii"
  fi
}

# Compute width safely for any terminal
calc_width() {
  local cols
  cols="$(tput cols 2>/dev/null || echo 80)"
  # Leave a small margin so we don't wrap
  local w=$((cols - 2))
  [[ "$w" -lt 20 ]] && w=20
  [[ "$w" -gt "$MAXW" ]] && w="$MAXW"
  echo "$w"
}

render_once() {
  local mode="$MODE"
  [[ "$mode" == "auto" ]] && mode="$(auto_mode)"

  local w
  w="$(calc_width)"

  local args=()
  # width: always set explicitly for stability across terminals
  args+=(-W "$w")

  if supports_color; then
    args+=(-C)
  fi

  if [[ "$mode" == "braille" ]]; then
    args+=(-b)
    [[ "$DITHER" -eq 1 ]] && args+=(--dither)
    args+=(--threshold "$THRESH")
  else
    # best general-purpose non-braille quality
    args+=(-c)
  fi

  [[ "$CLEAR" -eq 1 ]] && printf "\033[2J\033[H"  # clear + home
  ascii-image-converter "$IMG" "${args[@]}"
}

# Watch mode: re-render on resize; optionally on timer
if [[ "$WATCH" -eq 1 ]]; then
  need_redraw=1
  trap 'need_redraw=1' WINCH

  # Initial draw
  render_once
  need_redraw=0

  while :; do
    if [[ "$need_redraw" -eq 1 ]]; then
      render_once
      need_redraw=0
    fi
    if [[ "$RATE" -gt 0 ]]; then
      sleep "$RATE"
      need_redraw=1
    else
      # Low CPU idle
      sleep 0.2
    fi
  done
else
  render_once
fi
