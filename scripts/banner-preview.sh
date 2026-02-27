#!/usr/bin/env bash
# banner-preview.sh — Preview the fawx TUI banner with truecolor ANSI
# Run: ./scripts/banner-preview.sh

set -euo pipefail

# Truecolor escapes (24-bit)
GOLD="\x1b[38;2;255;215;0m"
AMBER="\x1b[38;2;255;165;0m"
BURNT="\x1b[38;2;210;112;10m"
DIM="\x1b[2m"
ITALIC="\x1b[3m"
BOLD="\x1b[1m"
RESET="\x1b[0m"

clear

echo ""
echo -e "${BOLD}${GOLD}   ___                   ${RESET}"
echo -e "${BOLD}${GOLD}  / _/__ __    __  _  _  ${RESET}"
echo -e "${BOLD}${GOLD} / _/ _ \`/ |/|/ /\\ \\/ / ${RESET}"
echo -e "${BOLD}${GOLD}/_/ \\_,_/|__,__/  >  <  ${RESET}"
echo ""
echo -e "${DIM}${AMBER}  agentic engine · type /help for commands${RESET}"
echo ""
echo -e "${BURNT}${DIM}  v0.1.0 · claude-sonnet-4 · anthropic (subscription)${RESET}"
echo ""
echo -e "${GOLD}you › ${RESET}what's the weather like?"
echo ""
echo -e "${AMBER}assistant › ${RESET}Let me check that for you."
echo ""
echo -e "${DIM}${BURNT}  ↳ 1 iteration · 153 in / 102 out tokens · 1.2s${RESET}"
echo ""
