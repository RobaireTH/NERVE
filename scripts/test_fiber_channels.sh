#!/usr/bin/env bash
# Test Fiber payment channel lifecycle: connect → open → pay → close.
#
# Requires two running Fiber nodes (poster and worker).
# Uses the MCP HTTP bridge /fiber/* endpoints.
#
# Prerequisites:
#   - nerve-mcp running: MCP_URL=http://localhost:8081
#   - Two Fiber nodes running with FIBER_RPC_URL and FIBER_WORKER_RPC_URL set
#   - Both Fiber nodes funded with testnet CKB
#
# Usage:
#   source .env && source .env.fiber && ./scripts/test_fiber_channels.sh

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
FIBER_WORKER_RPC_URL="${FIBER_WORKER_RPC_URL:-http://localhost:9227}"
FUNDING_CKB="${TEST_FUNDING_CKB:-100}"
PAYMENT_CKB="${TEST_PAYMENT_CKB:-1}"

step()  { echo; echo "── $* ──"; }
ok()    { echo "   OK: $*"; }
fail()  { echo "   FAIL: $*" >&2; exit 1; }
jget()  { echo "$1" | grep -o "\"$2\":\"[^\"]*\"" | cut -d'"' -f4 || true; }

step "Health check"
curl -sf "$MCP_URL/health" | grep -q '"status":"ok"' || fail "nerve-mcp not healthy"
ok "nerve-mcp healthy"

step "Poster: reading Fiber node info"
POSTER_INFO=$(curl -sf "$MCP_URL/fiber/node")
POSTER_NODE_ID=$(jget "$POSTER_INFO" "node_id")
[[ -n "$POSTER_NODE_ID" ]] || fail "Could not read poster node_id from $MCP_URL/fiber/node"
ok "Poster node_id: $POSTER_NODE_ID"

step "Worker: reading Fiber node info"
WORKER_INFO=$(curl -sf -X POST "$FIBER_WORKER_RPC_URL" \
	-H "Content-Type: application/json" \
	-d '{"jsonrpc":"2.0","id":1,"method":"node_info","params":[]}')
WORKER_NODE_ID=$(echo "$WORKER_INFO" | grep -o '"node_id":"[^"]*"' | cut -d'"' -f4 || true)
WORKER_ADDR=$(echo "$WORKER_INFO" | grep -o '"addresses":\["[^"]*"' | grep -o '"[^"]*"$' | tr -d '"' || true)
[[ -n "$WORKER_NODE_ID" ]] || fail "Could not read worker node_id from $FIBER_WORKER_RPC_URL"
ok "Worker node_id: $WORKER_NODE_ID"
ok "Worker addr: $WORKER_ADDR"

step "Poster: connecting to worker peer"
CONNECT_RESP=$(curl -sf -X POST "$MCP_URL/fiber/peers" \
	-H "Content-Type: application/json" \
	-d "{\"peer_address\": \"$WORKER_ADDR\"}")
ok "Connected: $CONNECT_RESP"

sleep 2

step "Poster: opening channel ($FUNDING_CKB CKB) with worker"
OPEN_RESP=$(curl -sf -X POST "$MCP_URL/fiber/channels" \
	-H "Content-Type: application/json" \
	-d "{\"peer_id\": \"$WORKER_NODE_ID\", \"funding_ckb\": $FUNDING_CKB}")
TEMP_CHANNEL_ID=$(jget "$OPEN_RESP" "temporary_channel_id")
[[ -n "$TEMP_CHANNEL_ID" ]] || fail "open channel failed: $OPEN_RESP"
ok "Temp channel ID: $TEMP_CHANNEL_ID"

echo "   Waiting 30s for channel to be funded and confirmed..."
sleep 30

step "Listing channels"
CHANNELS=$(curl -sf "$MCP_URL/fiber/channels")
echo "$CHANNELS" | grep -q "ChannelReady" || echo "   Warning: channel may still be opening"
CHANNEL_ID=$(echo "$CHANNELS" | grep -o '"channel_id":"[^"]*"' | head -1 | cut -d'"' -f4 || true)
[[ -n "$CHANNEL_ID" ]] || CHANNEL_ID="$TEMP_CHANNEL_ID"
ok "Channel ID: $CHANNEL_ID"

step "Worker: creating invoice for $PAYMENT_CKB CKB"
INVOICE_RESP=$(curl -sf -X POST "$FIBER_WORKER_RPC_URL" \
	-H "Content-Type: application/json" \
	-d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"new_invoice\",\"params\":{\"amount\":$(echo "$PAYMENT_CKB * 100000000" | bc | cut -d. -f1),\"description\":\"test payment\",\"currency\":\"Fibt\",\"expiry\":3600,\"final_expiry_delta\":86400000}}")
INVOICE=$(echo "$INVOICE_RESP" | grep -o '"invoice_address":"[^"]*"' | cut -d'"' -f4 || true)
[[ -n "$INVOICE" ]] || fail "invoice creation failed: $INVOICE_RESP"
ok "Invoice: ${INVOICE:0:40}..."

step "Poster: paying invoice ($PAYMENT_CKB CKB)"
PAY_RESP=$(curl -sf -X POST "$MCP_URL/fiber/pay" \
	-H "Content-Type: application/json" \
	-d "{\"invoice\": \"$INVOICE\"}")
PAY_STATUS=$(jget "$PAY_RESP" "status")
PAY_HASH=$(jget "$PAY_RESP" "payment_hash")
[[ "$PAY_STATUS" == "Success" ]] || fail "payment failed (status=$PAY_STATUS): $PAY_RESP"
ok "Payment hash: $PAY_HASH"
ok "Status: $PAY_STATUS"

step "Poster: closing channel $CHANNEL_ID"
CLOSE_RESP=$(curl -sf -X DELETE "$MCP_URL/fiber/channels/$CHANNEL_ID")
ok "Close response: $CLOSE_RESP"

echo
echo "Fiber Channel Test PASSED"
echo "  channel:  $CHANNEL_ID"
echo "  payment:  $PAY_HASH ($PAYMENT_CKB CKB)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
