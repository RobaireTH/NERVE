#!/usr/bin/env bash
# test_telegram_demo.sh — Validate services are running and print Telegram test instructions.
#
# Prerequisites:
#   - nerve-core running on :8080
#   - nerve-mcp bridge running on :8081
#   - OPENCLAW_TELEGRAM_TOKEN set in environment
#   - Contracts deployed: source .env.deployed
#
# Usage:
#   source .env && source .env.deployed && ./scripts/test_telegram_demo.sh

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
MCP_URL="${MCP_URL:-http://localhost:8081}"

step()    { echo; echo "── $* ──"; }
ok()      { echo "   ✓ $*"; }
fail()    { echo "   ✗ $*" >&2; exit 1; }

# ── Service Health Checks ────────────────────────────────────────────────────

step "Checking nerve-core ($CORE_URL)"
curl -sf "$CORE_URL/health" | grep -q '"status":"ok"' || fail "nerve-core not running on $CORE_URL"
ok "nerve-core healthy"

step "Checking nerve-mcp bridge ($MCP_URL)"
curl -sf "$MCP_URL/health" | grep -q '"status":"ok"' || fail "nerve-mcp not running on $MCP_URL"
ok "nerve-mcp healthy"

# ── Contract Deployment Check ────────────────────────────────────────────────

step "Checking deployed contracts"
[[ -n "${JOB_CELL_TYPE_CODE_HASH:-}" ]] || fail "JOB_CELL_TYPE_CODE_HASH not set"
ok "Job cell type script: ${JOB_CELL_TYPE_CODE_HASH:0:18}..."

if [[ -n "${MOCK_AMM_TYPE_CODE_HASH:-}" ]]; then
	ok "Mock AMM type script: ${MOCK_AMM_TYPE_CODE_HASH:0:18}..."
else
	echo "   ⚠ MOCK_AMM_TYPE_CODE_HASH not set — DeFi swap demos will fail"
fi

# ── Telegram Config Check ────────────────────────────────────────────────────

step "Checking Telegram config"
if [[ -n "${OPENCLAW_TELEGRAM_TOKEN:-}" ]]; then
	ok "OPENCLAW_TELEGRAM_TOKEN is set"
else
	fail "OPENCLAW_TELEGRAM_TOKEN not set — configure in .env"
fi

# ── Balance Check ────────────────────────────────────────────────────────────

step "Agent balance"
BALANCE=$(curl -sf "$CORE_URL/agent/balance")
LOCK_ARGS=$(echo "$BALANCE" | grep -o '"lock_args":"[^"]*"' | cut -d'"' -f4)
BAL_CKB=$(echo "$BALANCE" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)
echo "   Lock args: $LOCK_ARGS"
echo "   Balance:   $BAL_CKB CKB"

# ── Post a Test Job for Heartbeat ────────────────────────────────────────────

step "Posting a test job (5 CKB, 200 blocks TTL)"
CAPABILITY="0x0000000000000000000000000000000000000000000000000000000000000000"
POST_RESP=$(curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"post_job\",\"reward_ckb\":5,\"ttl_blocks\":200,\"capability_hash\":\"$CAPABILITY\"}" 2>&1) || true
TX_HASH=$(echo "$POST_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

if [[ -n "$TX_HASH" ]]; then
	ok "Test job posted: $TX_HASH"
	echo "   Explorer: https://testnet.explorer.nervos.org/transaction/$TX_HASH"
else
	echo "   ⚠ Could not post test job (may need more CKB): $POST_RESP"
fi

# ── Instructions ─────────────────────────────────────────────────────────────

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Telegram Demo — Test Messages"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo
echo "Start the OpenClaw agent gateway, then send these messages"
echo "to your NERVE Telegram bot:"
echo
echo "  1. Check balance:"
echo "     \"What's my balance?\""
echo
echo "  2. List open jobs:"
echo "     \"Show me open jobs\""
echo
echo "  3. Post a job:"
echo "     \"Post a job for 3 CKB\""
echo
echo "  4. Claim a job (use a real tx_hash:index):"
echo "     \"Claim 0x<tx_hash>:0\""
echo
echo "  5. DeFi swap (requires mock AMM pool):"
echo "     \"Swap 10 CKB for TEST_TOKEN\""
echo
echo "  6. Check transaction status:"
echo "     \"Status of 0x<tx_hash>\""
echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
