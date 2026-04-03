#!/usr/bin/env bash
set -euo pipefail

# Run with:
#   ./scripts/spec-tests/h23-tool-grouping-spec-contract.sh
#
# This script validates contract-level requirements in:
#   docs/specs/h2-3-tool-grouping-spec.md

SPEC_FILE="${SPEC_FILE:-docs/specs/h2-3-tool-grouping-spec.md}"
CONSTITUTION_FILE="${CONSTITUTION_FILE:-docs/squad-constitution.md}"
MANIFEST_FILE="${MANIFEST_FILE:-scripts/spec-tests/h23-tool-grouping-contract-manifest.yaml}"

if [[ ! -f "$SPEC_FILE" ]]; then
  echo "missing spec file: $SPEC_FILE" >&2
  exit 1
fi

if [[ ! -f "$CONSTITUTION_FILE" ]]; then
  echo "missing constitution file: $CONSTITUTION_FILE" >&2
  exit 1
fi

if [[ ! -f "$MANIFEST_FILE" ]]; then
  echo "missing contract manifest file: $MANIFEST_FILE" >&2
  exit 1
fi

SEARCH_BIN="${SEARCH_BIN:-}"
if [[ -z "$SEARCH_BIN" ]]; then
  if command -v rg >/dev/null 2>&1; then
    SEARCH_BIN="rg"
  elif command -v grep >/dev/null 2>&1; then
    SEARCH_BIN="grep"
  else
    echo "missing required search tool: need 'rg' or 'grep'" >&2
    exit 1
  fi
fi

if [[ "$SEARCH_BIN" != "rg" && "$SEARCH_BIN" != "grep" ]]; then
  echo "invalid SEARCH_BIN: $SEARCH_BIN (expected 'rg' or 'grep')" >&2
  exit 1
fi

if ! command -v "$SEARCH_BIN" >/dev/null 2>&1; then
  echo "missing configured search tool: $SEARCH_BIN" >&2
  exit 1
fi

search_fixed_string_in_file() {
  local file="$1"
  local pattern="$2"
  if [[ "$SEARCH_BIN" == "rg" ]]; then
    rg -q --fixed-strings -- "$pattern" "$file"
  else
    grep -F -q -- "$pattern" "$file"
  fi
}

search_regex_in_file() {
  local file="$1"
  local pattern="$2"
  if [[ "$SEARCH_BIN" == "rg" ]]; then
    rg -q -- "$pattern" "$file"
  else
    grep -E -q -- "$pattern" "$file"
  fi
}

file_contains_regex() {
  local file="$1"
  local pattern="$2"
  search_regex_in_file "$file" "$pattern"
}

text_contains_fixed() {
  local text="$1"
  local pattern="$2"
  if [[ "$SEARCH_BIN" == "rg" ]]; then
    printf '%s\n' "$text" | rg -q --fixed-strings -- "$pattern"
  else
    printf '%s\n' "$text" | grep -F -q -- "$pattern"
  fi
}

text_contains_regex() {
  local text="$1"
  local pattern="$2"
  if [[ "$SEARCH_BIN" == "rg" ]]; then
    printf '%s\n' "$text" | rg -q -- "$pattern"
  else
    printf '%s\n' "$text" | grep -E -q -- "$pattern"
  fi
}

extract_section() {
  local file="$1"
  local heading="$2"
  awk -v heading="$heading" '
    function normalize_heading(text) {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", text)
      gsub(/[[:space:]]+/, " ", text)
      return text
    }
    BEGIN {
      expected_heading = normalize_heading(heading)
    }
    $0 ~ /^#+[[:space:]]+/ {
      level = 0
      while (substr($0, level + 1, 1) == "#") {
        level++
      }
      current = normalize_heading(substr($0, level + 1))
      sub(/^#*[[:space:]]+/, "", current)
      if (!in_section && current == expected_heading) {
        in_section = 1
        heading_level = level
        next
      }
      if (in_section && level <= heading_level) {
        exit
      }
    }
    in_section { print }
  ' "$file"
}

assert_in_section() {
  local file="$1"
  local heading="$2"
  local pattern="$3"
  local section
  section="$(extract_section "$file" "$heading")"
  if [[ -z "$section" ]]; then
    echo "missing section in file '$file': $heading" >&2
    exit 1
  fi
  if ! text_contains_fixed "$section" "$pattern"; then
    echo "expected pattern missing in section '$heading' of file '$file': $pattern" >&2
    exit 1
  fi
}

assert_regex_in_section() {
  local file="$1"
  local heading="$2"
  local regex="$3"
  local section
  section="$(extract_section "$file" "$heading")"
  if [[ -z "$section" ]]; then
    echo "missing section in file '$file': $heading" >&2
    exit 1
  fi
  if ! text_contains_regex "$section" "$regex"; then
    echo "expected regex missing in section '$heading' of file '$file': $regex" >&2
    exit 1
  fi
}

assert_heading_regex_in_file() {
  local file="$1"
  local heading_level="$2"
  local heading_regex="$3"
  local expected_heading="$4"
  local hashes
  hashes="$(printf '%*s' "$heading_level" '')"
  hashes="${hashes// /#}"
  local full_regex="^${hashes}[[:space:]]+${heading_regex}"
  if ! file_contains_regex "$file" "$full_regex"; then
    echo "expected heading missing in file '$file': ${hashes} ${expected_heading}" >&2
    exit 1
  fi
}

manifest_list_entries() {
  local key="$1"
  awk -v key="$key" '
    BEGIN { in_key = 0 }
    /^[[:space:]]*#/ { next }
    {
      line = $0
      if (in_key == 0) {
        if (line ~ ("^[[:space:]]*" key ":[[:space:]]*$")) {
          in_key = 1
        }
        next
      }

      if (line ~ "^[[:space:]]*[A-Za-z0-9_]+:[[:space:]]*$") {
        exit
      }

      if (line ~ "^[[:space:]]*-[[:space:]]+") {
        sub(/^[[:space:]]*-[[:space:]]+/, "", line)
        print line
      }
    }
  ' "$MANIFEST_FILE"
}

assert_manifest_fixed_patterns() {
  local file="$1"
  local key="$2"
  local pattern
  while IFS= read -r pattern; do
    [[ -z "$pattern" ]] && continue
    if ! search_fixed_string_in_file "$file" "$pattern"; then
      echo "expected pattern missing in file '$file': $pattern" >&2
      exit 1
    fi
  done < <(manifest_list_entries "$key")
}

assert_manifest_regex_patterns() {
  local file="$1"
  local key="$2"
  local pattern
  while IFS= read -r pattern; do
    [[ -z "$pattern" ]] && continue
    if ! search_regex_in_file "$file" "$pattern"; then
      echo "expected regex missing in file '$file': $pattern" >&2
      exit 1
    fi
  done < <(manifest_list_entries "$key")
}

assert_manifest_absent_patterns() {
  local file="$1"
  local key="$2"
  local pattern
  while IFS= read -r pattern; do
    [[ -z "$pattern" ]] && continue
    if search_fixed_string_in_file "$file" "$pattern"; then
      echo "unexpected pattern present in file '$file': $pattern" >&2
      exit 1
    fi
  done < <(manifest_list_entries "$key")
}

assert_manifest_heading_rules() {
  local file="$1"
  local key="$2"
  local entry
  local level
  local rest
  local regex
  local expected
  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    level="${entry%%@@@*}"
    rest="${entry#*@@@}"
    regex="${rest%%@@@*}"
    expected="${rest#*@@@}"
    if [[ "$entry" == "$level" || "$rest" == "$regex" || -z "$level" || -z "$regex" || -z "$expected" ]]; then
      echo "invalid heading rule in manifest '$MANIFEST_FILE' for key '$key': $entry" >&2
      exit 1
    fi
    assert_heading_regex_in_file "$file" "$level" "$regex" "$expected"
  done < <(manifest_list_entries "$key")
}

assert_manifest_section_fixed_patterns() {
  local file="$1"
  local heading="$2"
  local key="$3"
  local pattern
  while IFS= read -r pattern; do
    [[ -z "$pattern" ]] && continue
    assert_in_section "$file" "$heading" "$pattern"
  done < <(manifest_list_entries "$key")
}

assert_manifest_section_regex_patterns() {
  local file="$1"
  local heading="$2"
  local key="$3"
  local regex
  while IFS= read -r regex; do
    [[ -z "$regex" ]] && continue
    assert_regex_in_section "$file" "$heading" "$regex"
  done < <(manifest_list_entries "$key")
}

validate_json_semantics_block() {
  local heading="$1"
  local block_id="$2"
  local text="$3"
  local compact
  compact="$(printf '%s\n' "$text" | tr -d '[:space:]')"
  if text_contains_regex "$compact" '"activeCategories":\[[^]]*"NAVIGATION"[^]]*]' \
    && text_contains_regex "$compact" '"reasonCodes":\[[^]]*"user_disabled_navigation"[^]]*]'; then
    echo "inconsistent JSON example semantics in $heading example #$block_id: NAVIGATION active while user_disabled_navigation present" >&2
    exit 1
  fi
}

assert_valid_example_semantics() {
  local heading="$1"
  local section
  section="$(extract_section "$SPEC_FILE" "$heading")"
  if [[ -z "$section" ]]; then
    echo "missing section in file '$SPEC_FILE': $heading" >&2
    exit 1
  fi

  local json_block_index=0
  local current_json=""
  local in_json_fence=0
  local found_json_fence=0
  local line
  while IFS= read -r line; do
    if [[ "$in_json_fence" -eq 0 && "$line" =~ ^[[:space:]]*\`\`\`json[[:space:]]*$ ]]; then
      in_json_fence=1
      found_json_fence=1
      current_json=""
      continue
    fi

    if [[ "$in_json_fence" -eq 1 && "$line" =~ ^[[:space:]]*\`\`\`[[:space:]]*$ ]]; then
      in_json_fence=0
      json_block_index=$((json_block_index + 1))
      validate_json_semantics_block "$heading" "$json_block_index" "$current_json"
      current_json=""
      continue
    fi

    if [[ "$in_json_fence" -eq 1 ]]; then
      current_json+="$line"$'\n'
    fi
  done <<< "$section"

  if [[ "$in_json_fence" -eq 1 && -n "$current_json" ]]; then
    json_block_index=$((json_block_index + 1))
    validate_json_semantics_block "$heading" "$json_block_index" "$current_json"
  fi

  if [[ "$found_json_fence" -eq 0 ]]; then
    local example_index=0
    local current_example=""
    local found_example_blocks=0
    while IFS= read -r line; do
      if [[ "$line" =~ ^[[:space:]]*[0-9]+\.[[:space:]]+Example[[:space:]] ]]; then
        if [[ -n "$current_example" ]]; then
          validate_json_semantics_block "$heading" "$example_index" "$current_example"
        fi
        found_example_blocks=1
        example_index=$((example_index + 1))
        current_example="$line"$'\n'
        continue
      fi

      if [[ -n "$current_example" ]]; then
        current_example+="$line"$'\n'
      fi
    done <<< "$section"

    if [[ -n "$current_example" ]]; then
      validate_json_semantics_block "$heading" "$example_index" "$current_example"
    fi

    if [[ "$found_example_blocks" -eq 0 ]]; then
      validate_json_semantics_block "$heading" "1" "$section"
    fi
  fi
}

assert_manifest_fixed_patterns "$SPEC_FILE" "spec_required_fixed"
assert_manifest_regex_patterns "$SPEC_FILE" "spec_required_regex"
assert_manifest_absent_patterns "$SPEC_FILE" "spec_forbidden_fixed"
assert_manifest_heading_rules "$SPEC_FILE" "spec_required_headings"
assert_manifest_section_fixed_patterns "$SPEC_FILE" "7.3 Acceptance criteria" "spec_section_7_3_fixed"
assert_manifest_section_fixed_patterns "$SPEC_FILE" "7.4 ResolvedToolPlan examples (normative)" "spec_section_7_4_fixed"

assert_valid_example_semantics "7.4 ResolvedToolPlan examples (normative)"

assert_manifest_heading_rules "$CONSTITUTION_FILE" "constitution_required_headings"
assert_manifest_section_regex_patterns "$CONSTITUTION_FILE" "5.2 Policy Precedence" "constitution_section_5_2_regex"
assert_manifest_section_regex_patterns "$CONSTITUTION_FILE" "5.3 Determinism" "constitution_section_5_3_regex"
assert_manifest_section_regex_patterns "$CONSTITUTION_FILE" "7.3 Acceptance Criteria" "constitution_section_7_3_regex"
assert_manifest_section_regex_patterns "$CONSTITUTION_FILE" "8.1 Testing Baseline" "constitution_section_8_1_regex"
assert_manifest_section_regex_patterns "$CONSTITUTION_FILE" "8.6 Privacy Baseline" "constitution_section_8_6_regex"
assert_manifest_section_regex_patterns "$CONSTITUTION_FILE" "9. Rollout And Gates" "constitution_section_9_regex"

echo "h2-3 tool grouping spec contract checks passed"
