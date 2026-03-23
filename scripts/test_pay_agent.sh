#!/usr/bin/env bash
# Test the pay-agent flow through the MCP bridge.
#
# Requires:
#   - nerve-mcp running
#   - Fiber node ready on the payer side
#   - target agent identity cell live on-chain
#   - a usable Fiber route/channel to the target pubkey
#
# Usage:
#   ./scripts/test_pay_agent.sh --lock-args 0x... --amount-ckb 1 [--description "demo"]

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
LOCK_ARGS=""
AMOUNT_CKB="1"
DESCRIPTION="pay-agent demo"

step()  { echo; echo "── $* ──"; }
ok()    { echo "   OK: $*"; }
fail()  { echo "   FAIL: $*" >&2; exit 1; }
jget()  { echo "$1" | grep -o "\"$2\":\"[^\"]*\"" | cut -d'"' -f4 || true; }

while [[ $# -gt 0 ]]; do
	case "$1" in
		--lock-args)   LOCK_ARGS="$2";   shift 2 ;;
		--amount-ckb)  AMOUNT_CKB="$2";  shift 2 ;;
		--description) DESCRIPTION="$2"; shift 2 ;;
		*) fail "unknown argument: $1" ;;
	esac
done

[[ -n "$LOCK_ARGS" ]] || fail "--lock-args is required"

step "Health check"
curl -sf "$MCP_URL/health" | grep -q '"status":"ok"' || fail "nerve-mcp not healthy"
ok "nerve-mcp healthy"

step "Checking Fiber readiness"
READY_RESP=$(curl -sf "$MCP_URL/fiber/ready")
echo "$READY_RESP" | grep -q '"ready":true' || fail "Fiber node is not ready: $READY_RESP"
ok "Fiber layer ready"

step "Resolving target agent identity"
AGENT_RESP=$(curl -sf "$MCP_URL/agents/$LOCK_ARGS") || fail "target agent not found"
AGENT_PUBKEY=$(jget "$AGENT_RESP" "pubkey")
[[ -n "$AGENT_PUBKEY" ]] || fail "could not resolve agent pubkey: $AGENT_RESP"
ok "Resolved pubkey: $AGENT_PUBKEY"

step "Sending pay-agent keysend ($AMOUNT_CKB CKB)"
PAY_RESP=$(curl -sf -X POST "$MCP_URL/fiber/pay-agent" \
	-H "Content-Type: application/json" \
	-d "{\"lock_args\":\"$LOCK_ARGS\",\"amount_ckb\":$AMOUNT_CKB,\"description\":\"$DESCRIPTION\"}") || fail "pay-agent request failed"
PAY_HASH=$(jget "$PAY_RESP" "payment_hash")
PAY_STATUS=$(jget "$PAY_RESP" "status")
[[ -n "$PAY_HASH" ]] || fail "missing payment_hash: $PAY_RESP"
ok "Payment hash: $PAY_HASH"
ok "Status: ${PAY_STATUS:-unknown}"

echo
echo "Pay-Agent Test COMPLETE"
echo "  lock_args:    $LOCK_ARGS"
echo "  payment_hash: $PAY_HASH"
echo "  status:       ${PAY_STATUS:-unknown}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
