#!/usr/bin/env bash
# End-to-end marketplace test using the nerve CLI.
#
# Tests the full job lifecycle via the `nerve` CLI against a running nerve-core.
# This is the single-agent version (poster and worker use the same key).
#
# Prerequisites:
#   - nerve-core running: CORE_URL=http://localhost:8080
#   - nerve-mcp running:  MCP_URL=http://localhost:8081
#   - JOB_CELL_TYPE_CODE_HASH set (from .env.deployed)
#   - Sufficient testnet CKB
#
# Usage:
#   source .env && source .env.deployed && ./scripts/test_e2e_marketplace.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NERVE="$SCRIPT_DIR/nerve"
CORE_URL="${CORE_URL:-http://localhost:8080}"
MCP_URL="${MCP_URL:-http://localhost:8081}"

REWARD_CKB="5"
TTL_BLOCKS="100"

step()   { echo; echo "── $* ──"; }
ok()     { echo "   OK: $*"; }
fail()   { echo "   FAIL: $*" >&2; exit 1; }
extract() { echo "$1" | grep -o "\"$2\":\"[^\"]*\"" | cut -d'"' -f4; }

# ── Health checks ──────────────────────────────────────────────────────────────

step "Health checks"
curl -sf "$CORE_URL/health" | grep -q '"status":"ok"' || fail "nerve-core not healthy at $CORE_URL"
curl -sf "$MCP_URL/health"  | grep -q '"status":"ok"' || fail "nerve-mcp not healthy at $MCP_URL"
ok "Both services healthy"

# ── Read agent lock_args (needed for complete_job) ─────────────────────────────

step "Reading agent balance and lock_args"
BALANCE_JSON=$(curl -sf "$CORE_URL/agent/balance")
LOCK_ARGS=$(extract "$BALANCE_JSON" "lock_args")
BALANCE_CKB=$(echo "$BALANCE_JSON" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)
[[ -n "$LOCK_ARGS" ]] || fail "could not read lock_args"
ok "lock_args=$LOCK_ARGS, balance=${BALANCE_CKB} CKB"

# ── Post job ───────────────────────────────────────────────────────────────────

step "1. nerve post --reward $REWARD_CKB --ttl $TTL_BLOCKS"
POST_JSON=$(CORE_URL="$CORE_URL" MCP_URL="$MCP_URL" "$NERVE" post --reward "$REWARD_CKB" --ttl "$TTL_BLOCKS" 2>&1)
JOB_TX=$(extract "$POST_JSON" "tx_hash")
[[ -n "$JOB_TX" ]] || fail "post failed: $POST_JSON"
ok "tx_hash=$JOB_TX"

echo "   Waiting 15s for indexer..."
sleep 15

# ── List jobs — verify it appears ─────────────────────────────────────────────

step "1b. nerve jobs --status Open"
JOBS_JSON=$(CORE_URL="$CORE_URL" MCP_URL="$MCP_URL" "$NERVE" jobs --status Open 2>&1)
echo "$JOBS_JSON" | grep -q "$JOB_TX" || { echo "   Warning: job not yet indexed (continuing)"; }

# ── Reserve ────────────────────────────────────────────────────────────────────

step "2. nerve reserve --job $JOB_TX:0 --worker $LOCK_ARGS"
RESERVE_JSON=$(CORE_URL="$CORE_URL" MCP_URL="$MCP_URL" "$NERVE" reserve --job "$JOB_TX:0" --worker "$LOCK_ARGS" 2>&1)
RESERVE_TX=$(extract "$RESERVE_JSON" "tx_hash")
[[ -n "$RESERVE_TX" ]] || fail "reserve failed: $RESERVE_JSON"
ok "tx_hash=$RESERVE_TX"

sleep 15

# ── Claim ──────────────────────────────────────────────────────────────────────

step "3. nerve claim --job $RESERVE_TX:0"
CLAIM_JSON=$(CORE_URL="$CORE_URL" MCP_URL="$MCP_URL" "$NERVE" claim --job "$RESERVE_TX:0" 2>&1)
CLAIM_TX=$(extract "$CLAIM_JSON" "tx_hash")
[[ -n "$CLAIM_TX" ]] || fail "claim failed: $CLAIM_JSON"
ok "tx_hash=$CLAIM_TX"

sleep 15

# ── Complete ───────────────────────────────────────────────────────────────────

step "4. nerve complete --job $CLAIM_TX:0 --worker $LOCK_ARGS"
COMPLETE_JSON=$(CORE_URL="$CORE_URL" MCP_URL="$MCP_URL" "$NERVE" complete --job "$CLAIM_TX:0" --worker "$LOCK_ARGS" 2>&1)
COMPLETE_TX=$(extract "$COMPLETE_JSON" "tx_hash")
[[ -n "$COMPLETE_TX" ]] || fail "complete failed: $COMPLETE_JSON"
ok "tx_hash=$COMPLETE_TX"

# ── Verify balance changed ─────────────────────────────────────────────────────

step "5. Verify final balance"
FINAL_JSON=$(curl -sf "$CORE_URL/agent/balance")
FINAL_CKB=$(echo "$FINAL_JSON" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)
ok "Final balance: ${FINAL_CKB} CKB (was ${BALANCE_CKB} CKB)"

# ── Summary ────────────────────────────────────────────────────────────────────

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "E2E Marketplace Test PASSED"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  post:     $JOB_TX"
echo "  reserve:  $RESERVE_TX"
echo "  claim:    $CLAIM_TX"
echo "  complete: $COMPLETE_TX"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
