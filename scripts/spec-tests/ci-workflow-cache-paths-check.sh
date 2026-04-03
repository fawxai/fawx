#!/usr/bin/env bash
set -euo pipefail

WORKFLOW_FILE="${WORKFLOW_FILE:-.github/workflows/ci.yml}"
RULE_ID="H23-CI-CACHE-001"

if [[ ! -f "$WORKFLOW_FILE" ]]; then
  echo "missing workflow file: $WORKFLOW_FILE" >&2
  exit 1
fi

if ! command -v awk >/dev/null 2>&1; then
  echo "missing required search tool: awk" >&2
  exit 1
fi

unsafe_matches="$(
  awk '
    function ltrim(s) { sub(/^[[:space:]]+/, "", s); return s }
    function rtrim(s) { sub(/[[:space:]]+$/, "", s); return s }
    function trim(s)  { return rtrim(ltrim(s)) }
    function indent_len(s, t) { t = s; sub(/[^[:space:]].*$/, "", t); return length(t) }
    function is_inline_list(v) {
      v = trim(v)
      sub(/^-+[[:space:]]*/, "", v)
      return v ~ /^\[[^][]*\][[:space:]]*(#.*)?$/
    }
    function emit_unsafe_from_inline_list(v, line_no, inner, n, i, item) {
      inner = trim(v)
      sub(/^-+[[:space:]]*/, "", inner)
      sub(/^\[[[:space:]]*/, "", inner)
      sub(/[[:space:]]*\][[:space:]]*(#.*)?$/, "", inner)
      n = split(inner, items, /,[[:space:]]*/)
      for (i = 1; i <= n; i++) {
        item = trim(items[i])
        if (item != "" && is_unsafe_path(item)) {
          print line_no ":" item
        }
      }
    }
    function emit_unsafe_from_path_value(v, line_no) {
      if (is_inline_list(v)) {
        emit_unsafe_from_inline_list(v, line_no)
      } else if (is_unsafe_path(v)) {
        print line_no ":" trim(v)
      }
    }
    function reset_step_state(i) {
      in_step = 0
      step_indent = -1
      in_cache_step = 0
      in_path_block = 0
      path_indent = -1
      for (i = 1; i <= step_path_count; i++) {
        delete step_path_line[i]
        delete step_path_value[i]
      }
      step_path_count = 0
    }
    function collect_step_path(v, line_no) {
      step_path_count++
      step_path_line[step_path_count] = line_no
      step_path_value[step_path_count] = v
    }
    function flush_step_paths(i) {
      if (in_cache_step) {
        for (i = 1; i <= step_path_count; i++) {
          emit_unsafe_from_path_value(step_path_value[i], step_path_line[i])
        }
      }
      for (i = 1; i <= step_path_count; i++) {
        delete step_path_line[i]
        delete step_path_value[i]
      }
      step_path_count = 0
    }
    function is_unsafe_path(v) {
      v = trim(v)
      sub(/^-+[[:space:]]*/, "", v)
      gsub(/^["'"'"']|["'"'"']$/, "", v)
      return v ~ /^\/(var|etc|usr|opt|root)(\/|$)/
    }

    BEGIN {
      in_steps_block = 0
      steps_indent = -1
      in_step = 0
      step_indent = -1
      in_cache_step = 0
      in_path_block = 0
      path_indent = -1
      step_path_count = 0
    }

    {
      line = $0
      current_indent = indent_len(line)
      line_trimmed = trim(line)

      if (line ~ /^[[:space:]]*steps:[[:space:]]*($|#)/) {
        flush_step_paths()
        in_steps_block = 1
        steps_indent = current_indent
        reset_step_state()
      } else if (in_steps_block && line_trimmed != "" && current_indent <= steps_indent) {
        flush_step_paths()
        in_steps_block = 0
        reset_step_state()
      }

      # Step context: only enforce path checks for actions/cache steps.
      if (in_step && current_indent <= step_indent && line !~ /^[[:space:]]*-[[:space:]]/) {
        flush_step_paths()
        reset_step_state()
      }

      if (in_steps_block && line ~ /^[[:space:]]*-[[:space:]]/ && current_indent > steps_indent && (!in_step || current_indent == step_indent)) {
        if (in_step) {
          flush_step_paths()
        }
        in_step = 1
        step_indent = current_indent
        in_cache_step = 0
        in_path_block = 0
        path_indent = -1
        step_path_count = 0
      }

      if (in_step && (line ~ /^[[:space:]]*-[[:space:]]*uses:[[:space:]]*["'"'"']?actions\/cache(\/(restore|save))?@/ || line ~ /^[[:space:]]*uses:[[:space:]]*["'"'"']?actions\/cache(\/(restore|save))?@/)) {
        in_cache_step = 1
      }

      if (in_path_block) {
        if (line_trimmed == "") {
          next
        }

        if (current_indent <= path_indent) {
          in_path_block = 0
        } else {
          collect_step_path(line_trimmed, NR)
          next
        }
      }

      if (in_step && line ~ /^[[:space:]]*path:[[:space:]]*/) {
        path_indent = indent_len(line)
        value = line
        sub(/^[[:space:]]*path:[[:space:]]*/, "", value)
        value = trim(value)

        if (value == "" || value ~ /^[|>]/) {
          in_path_block = 1
        } else {
          collect_step_path(value, NR)
        }
      }
    }
    END {
      flush_step_paths()
    }
  ' "$WORKFLOW_FILE"
)"

if [[ -n "$unsafe_matches" ]]; then
  echo "[$RULE_ID] disallowed cache path detected in $WORKFLOW_FILE: cache paths must be user-writable" >&2
  echo "$unsafe_matches" >&2
  exit 1
fi

echo "[$RULE_ID] ci workflow cache path safety checks passed"
