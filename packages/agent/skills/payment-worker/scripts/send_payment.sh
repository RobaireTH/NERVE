#!/usr/bin/env bash
# Send a Fiber payment via invoice or keysend.
#
# Usage (invoice):  send_payment.sh --invoice fibt1...
# Usage (keysend):  send_payment.sh --target-pubkey 0x... --amount-ckb 5
# Output: JSON with payment_hash and status.
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
INVOICE=""
TARGET_PUBKEY=""
AMOUNT_CKB=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--invoice)       INVOICE="$2";       shift 2 ;;
		--target-pubkey) TARGET_PUBKEY="$2"; shift 2 ;;
		--amount-ckb)    AMOUNT_CKB="$2";    shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -n "$INVOICE" ]]; then
	BODY="{\"invoice\": \"$INVOICE\"}"
elif [[ -n "$TARGET_PUBKEY" && -n "$AMOUNT_CKB" ]]; then
	BODY="{\"target_pubkey\": \"$TARGET_PUBKEY\", \"amount_ckb\": $AMOUNT_CKB}"
else
	echo '{"error": "provide --invoice or (--target-pubkey + --amount-ckb)"}' >&2
	exit 1
fi

curl -sf -X POST "$MCP_URL/fiber/pay" \
	-H "Content-Type: application/json" \
	-d "$BODY"
