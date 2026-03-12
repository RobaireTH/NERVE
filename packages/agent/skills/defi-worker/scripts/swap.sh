#!/usr/bin/env bash
# Execute a CKB/token swap via the TX Builder.
#
# Usage: swap.sh --from-asset CKB --to-asset TEST_TOKEN --amount-ckb 10 [--slippage-bps 100]
# Output: JSON with tx_hash on success.
#
# Environment:
#   CORE_URL (default: http://localhost:8080)

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
FROM_ASSET="CKB"
TO_ASSET=""
AMOUNT_CKB=""
SLIPPAGE_BPS="100"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--from-asset)    FROM_ASSET="$2";    shift 2 ;;
		--to-asset)      TO_ASSET="$2";      shift 2 ;;
		--amount-ckb)    AMOUNT_CKB="$2";    shift 2 ;;
		--slippage-bps)  SLIPPAGE_BPS="$2";  shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$TO_ASSET" || -z "$AMOUNT_CKB" ]]; then
	echo '{"error": "--to-asset and --amount-ckb are required"}' >&2
	exit 1
fi

curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{
		\"intent\": \"swap\",
		\"from_asset\": \"$FROM_ASSET\",
		\"to_asset\": \"$TO_ASSET\",
		\"amount_ckb\": $AMOUNT_CKB,
		\"slippage_bps\": $SLIPPAGE_BPS
	}"
