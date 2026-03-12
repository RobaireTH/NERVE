#!/usr/bin/env bash
# Poll transaction confirmation status.
#
# Usage: get_tx_status.sh --tx-hash 0x...
# Output: JSON with transaction status.
#
# Environment:
#   CORE_URL (default: http://localhost:8080)

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
TX_HASH=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--tx-hash) TX_HASH="$2"; shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$TX_HASH" ]]; then
	echo '{"error": "--tx-hash is required"}' >&2
	exit 1
fi

curl -sf "$CORE_URL/tx/status?tx_hash=$TX_HASH"
