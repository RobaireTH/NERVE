#!/usr/bin/env bash
# Poll a transaction until it reaches "committed" status or times out.
#
# Usage: wait_for_tx.sh --tx-hash 0x... [--max-polls 20] [--interval 5]
# Exit 0 on committed, exit 1 on timeout or error.
#
# Environment:
#   CORE_URL (default: http://localhost:8080)

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
TX_HASH=""
MAX_POLLS="20"
INTERVAL="5"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--tx-hash)    TX_HASH="$2";    shift 2 ;;
		--max-polls)  MAX_POLLS="$2";  shift 2 ;;
		--interval)   INTERVAL="$2";   shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$TX_HASH" ]]; then
	echo '{"error": "--tx-hash is required"}' >&2
	exit 1
fi

for i in $(seq 1 "$MAX_POLLS"); do
	RESP=$(curl -sf "$CORE_URL/tx/status?tx_hash=$TX_HASH" 2>/dev/null || echo '{"error":"rpc unavailable"}')
	STATUS=$(echo "$RESP" | grep -o '"min_replace_fee":' | head -1 || true)

	# The CKB get_transaction result nests status inside transaction_status.
	TX_STATUS=$(echo "$RESP" | grep -o '"tx_status":"[^"]*"' | cut -d'"' -f4 || true)

	if [[ "$TX_STATUS" == "committed" ]]; then
		echo "{\"status\": \"committed\", \"tx_hash\": \"$TX_HASH\", \"polls\": $i}"
		exit 0
	fi

	if [[ "$TX_STATUS" == "rejected" ]]; then
		echo "{\"status\": \"rejected\", \"tx_hash\": \"$TX_HASH\", \"polls\": $i}" >&2
		exit 1
	fi

	echo "  poll $i/$MAX_POLLS: status: ${TX_STATUS:-unknown}, waiting ${INTERVAL}s..." >&2
	sleep "$INTERVAL"
done

echo "{\"error\": \"timed out after $MAX_POLLS polls\", \"tx_hash\": \"$TX_HASH\"}" >&2
exit 1
