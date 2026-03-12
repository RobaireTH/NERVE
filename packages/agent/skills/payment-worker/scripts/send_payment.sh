#!/usr/bin/env bash
# Send a payment over an open Fiber channel.
#
# Usage: send_payment.sh --channel-id <id> --amount-ckb 5 [--description "..."]
# Output: JSON with payment result.
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
CHANNEL_ID=""
AMOUNT_CKB=""
DESCRIPTION=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--channel-id)  CHANNEL_ID="$2";   shift 2 ;;
		--amount-ckb)  AMOUNT_CKB="$2";   shift 2 ;;
		--description) DESCRIPTION="$2";  shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$CHANNEL_ID" || -z "$AMOUNT_CKB" ]]; then
	echo '{"error": "--channel-id and --amount-ckb are required"}' >&2
	exit 1
fi

curl -sf -X POST "$MCP_URL/fiber/channels/$CHANNEL_ID/pay" \
	-H "Content-Type: application/json" \
	-d "{\"amount_ckb\": $AMOUNT_CKB, \"description\": \"$DESCRIPTION\"}"
