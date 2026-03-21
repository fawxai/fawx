#!/bin/bash
# Mac Mini / MacBook cleanup for Fawx development
# Clears build caches, stale branches, and temp files that pile up
# Run: bash scripts/cleanup-mac.sh [--dry-run]

set -euo pipefail

DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

freed=0

log() { echo "  $1"; }
header() { echo ""; echo "=== $1 ==="; }

size_mb() {
    local path="$1"
    if [[ -e "$path" ]]; then
        du -sm "$path" 2>/dev/null | awk '{print $1}'
    else
        echo 0
    fi
}

clean() {
    local path="$1"
    local label="$2"
    local mb
    mb=$(size_mb "$path")
    if [[ "$mb" -gt 0 ]]; then
        if $DRY_RUN; then
            log "[dry-run] Would remove $label (${mb}MB): $path"
        else
            rm -rf "$path"
            log "Removed $label (${mb}MB)"
        fi
        freed=$((freed + mb))
    fi
}

header "Xcode DerivedData"
# DerivedData is the #1 disk hog. Xcode rebuilds it as needed.
DERIVED="$HOME/Library/Developer/Xcode/DerivedData"
for dir in "$DERIVED"/Fawx-*; do
    [[ -d "$dir" ]] && clean "$dir" "$(basename "$dir")"
done

header "Cargo build cache"
FAWX_DIR="$HOME/fawx"
if [[ -d "$FAWX_DIR/target" ]]; then
    mb=$(size_mb "$FAWX_DIR/target")
    log "Cargo target: ${mb}MB"
    # Only clean if over 5GB
    if [[ "$mb" -gt 5120 ]]; then
        if $DRY_RUN; then
            log "[dry-run] Would run: cargo clean in $FAWX_DIR"
        else
            (cd "$FAWX_DIR" && cargo clean 2>/dev/null)
            log "Ran cargo clean"
        fi
        freed=$((freed + mb))
    else
        log "Under 5GB threshold, skipping"
    fi
fi

header "SPM caches"
SPM_CACHE="$HOME/Library/Caches/org.swift.swiftpm"
clean "$SPM_CACHE" "SPM cache"

header "Xcode caches"
clean "$HOME/Library/Caches/com.apple.dt.Xcode" "Xcode cache"

header "Test temp files"
for pattern in fawx-api-tests fawx-headless-tests; do
    count=$(find /tmp -maxdepth 1 -name "${pattern}-*" 2>/dev/null | wc -l)
    if [[ "$count" -gt 0 ]]; then
        if $DRY_RUN; then
            log "[dry-run] Would remove $count $pattern dirs"
        else
            rm -rf /tmp/${pattern}-*
            log "Removed $count $pattern dirs"
        fi
    fi
done

header "Stale git branches"
if [[ -d "$FAWX_DIR/.git" ]]; then
    cd "$FAWX_DIR"
    stale=$(git branch | grep -v 'dev\|main\|staging\|\*' | wc -l)
    if [[ "$stale" -gt 0 ]]; then
        if $DRY_RUN; then
            log "[dry-run] Would delete $stale local branches"
            git branch | grep -v 'dev\|main\|staging\|\*'
        else
            git branch | grep -v 'dev\|main\|staging\|\*' | xargs git branch -D 2>/dev/null || true
            log "Deleted $stale stale branches"
        fi
    else
        log "No stale branches"
    fi
    git remote prune origin 2>/dev/null
    log "Pruned remote tracking refs"
fi

header "Stale worktrees"
if [[ -d "$FAWX_DIR/.git" ]]; then
    cd "$FAWX_DIR"
    wt_count=$(git worktree list | grep -v "$FAWX_DIR " | wc -l)
    if [[ "$wt_count" -gt 0 ]]; then
        git worktree list | grep -v "$FAWX_DIR "
        if ! $DRY_RUN; then
            git worktree prune
            log "Pruned stale worktrees"
        fi
    else
        log "No stale worktrees"
    fi
fi

header "Homebrew cache"
BREW_CACHE="$HOME/Library/Caches/Homebrew"
mb=$(size_mb "$BREW_CACHE")
if [[ "$mb" -gt 500 ]]; then
    if $DRY_RUN; then
        log "[dry-run] Would run: brew cleanup (${mb}MB)"
    else
        brew cleanup --prune=7 2>/dev/null
        log "Ran brew cleanup"
    fi
    freed=$((freed + mb / 2))  # estimate half freed
else
    log "Homebrew cache: ${mb}MB (under 500MB threshold)"
fi

header "Summary"
if $DRY_RUN; then
    echo "Would free approximately ${freed}MB"
    echo "Run without --dry-run to actually clean"
else
    echo "Freed approximately ${freed}MB"
fi
