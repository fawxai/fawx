#!/bin/bash
# Phase 5 endpoint smoke test
# Usage: ./scripts/smoke-test-endpoints.sh [base_url] [bearer_token]
# Defaults: http://localhost:8400 and reads from ~/.fawx/auth.db

set -euo pipefail

BASE="${1:-http://localhost:8400}"
TOKEN="${2:-test-token}"
AUTH="Authorization: Bearer $TOKEN"
PASS=0
FAIL=0
TOTAL=0

check() {
    local desc="$1"
    local expected_status="$2"
    local method="$3"
    local path="$4"
    local body="${5:-}"

    TOTAL=$((TOTAL + 1))

    if [ -n "$body" ]; then
        actual=$(curl -s -o /dev/null -w "%{http_code}" -X "$method" \
            -H "$AUTH" -H "Content-Type: application/json" \
            -d "$body" "${BASE}${path}" 2>/dev/null)
    else
        actual=$(curl -s -o /dev/null -w "%{http_code}" -X "$method" \
            -H "$AUTH" "${BASE}${path}" 2>/dev/null)
    fi

    if [ "$actual" = "$expected_status" ]; then
        echo "  ✅ $desc (HTTP $actual)"
        PASS=$((PASS + 1))
    else
        echo "  ❌ $desc (expected $expected_status, got $actual)"
        FAIL=$((FAIL + 1))
    fi
}

check_body() {
    local desc="$1"
    local method="$2"
    local path="$3"
    local field="$4"
    local body="${5:-}"

    TOTAL=$((TOTAL + 1))

    if [ -n "$body" ]; then
        response=$(curl -s -X "$method" \
            -H "$AUTH" -H "Content-Type: application/json" \
            -d "$body" "${BASE}${path}" 2>/dev/null)
    else
        response=$(curl -s -X "$method" \
            -H "$AUTH" "${BASE}${path}" 2>/dev/null)
    fi

    if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('$field','MISSING'))" 2>/dev/null | grep -qv "MISSING"; then
        echo "  ✅ $desc ($field present)"
        PASS=$((PASS + 1))
    else
        echo "  ❌ $desc ($field missing in response)"
        echo "     Response: $(echo "$response" | head -c 200)"
        FAIL=$((FAIL + 1))
    fi
}

echo ""
echo "🦝 Fawx Phase 5 Endpoint Smoke Test"
echo "   Target: $BASE"
echo ""

echo "── Core ──"
check "Health endpoint" "200" "GET" "/health"
check "Status (authed)" "200" "GET" "/status"

echo ""
echo "── Permissions API ──"
check "GET /v1/permissions" "200" "GET" "/v1/permissions"
check_body "Permissions has preset field" "GET" "/v1/permissions" "preset"
check "PATCH /v1/permissions (apply preset)" "200" "PATCH" "/v1/permissions" '{"preset":"cautious"}'
check "PATCH /v1/permissions (restore power)" "200" "PATCH" "/v1/permissions" '{"preset":"power"}'
check "PATCH /v1/permissions (invalid action)" "422" "PATCH" "/v1/permissions" '{"changes":[{"action":"nonexistent","level":"allow"}]}'

echo ""
echo "── Synthesis CRUD ──"
check "GET /v1/synthesis" "200" "GET" "/v1/synthesis"
check_body "Synthesis has max_length" "GET" "/v1/synthesis" "max_length"
check "PUT /v1/synthesis" "200" "PUT" "/v1/synthesis" '{"synthesis":"Be concise and direct."}'
check "PUT /v1/synthesis (too long)" "422" "PUT" "/v1/synthesis" "{\"synthesis\":\"$(python3 -c "print('x'*501)")\"}"
check "DELETE /v1/synthesis" "200" "DELETE" "/v1/synthesis"

echo ""
echo "── OAuth ──"
check "GET /v1/auth/openai/oauth-start" "200" "GET" "/v1/auth/openai/oauth-start"
check "GET /v1/auth/anthropic/oauth-start (unsupported)" "400" "GET" "/v1/auth/anthropic/oauth-start"
check "POST /v1/auth/openai/oauth-callback (bad token)" "400" "POST" "/v1/auth/openai/oauth-callback" '{"code":"test","flow_token":"invalid"}'
check "POST /v1/auth/openai/refresh (no creds)" "404" "POST" "/v1/auth/openai/refresh"

echo ""
echo "── Marketplace ──"
check "GET /v1/skills/search" "200" "GET" "/v1/skills/search?q=test"
check_body "Search returns marketplace_available" "GET" "/v1/skills/search?q=test" "marketplace_available"
check "POST /v1/skills/install (stub)" "503" "POST" "/v1/skills/install" '{"name":"test-skill"}'
check "DELETE /v1/skills/test (not found)" "404" "DELETE" "/v1/skills/test-skill"

echo ""
echo "── Permission Prompts ──"
check "POST /v1/permissions/prompts/invalid/respond (not found)" "404" "POST" "/v1/permissions/prompts/invalid/respond" '{"decision":"allow"}'
check "POST /v1/permissions/prompts/invalid/respond (bad decision)" "422" "POST" "/v1/permissions/prompts/test/respond" '{"decision":"maybe"}'

echo ""
echo "── Fleet Dashboard ──"
check "GET /v1/fleet/overview (no fleet)" "503" "GET" "/v1/fleet/overview"
check "GET /v1/fleet/nodes (no fleet)" "503" "GET" "/v1/fleet/nodes"

echo ""
echo "── Proposals ──"
check "GET /v1/proposals/pending" "200" "GET" "/v1/proposals/pending"
check "GET /v1/proposals/history" "200" "GET" "/v1/proposals/history"

echo ""
echo "── Usage ──"
check "GET /v1/usage" "200" "GET" "/v1/usage"

echo ""
echo "════════════════════════════"
echo "  Results: $PASS passed, $FAIL failed, $TOTAL total"
echo "════════════════════════════"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
