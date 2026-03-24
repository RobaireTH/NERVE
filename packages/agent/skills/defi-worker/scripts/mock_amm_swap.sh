#!/usr/bin/env bash

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
POOL_TX_HASH="${POOL_TX_HASH:-}"
POOL_INDEX="${POOL_INDEX:-0}"
AMOUNT_CKB=""
SLIPPAGE_BPS="100"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--pool-tx-hash) POOL_TX_HASH="$2"; shift 2 ;;
		--pool-index)   POOL_INDEX="$2"; shift 2 ;;
		--amount-ckb)   AMOUNT_CKB="$2"; shift 2 ;;
		--slippage-bps) SLIPPAGE_BPS="$2"; shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$POOL_TX_HASH" || -z "$AMOUNT_CKB" ]]; then
	echo '{"error":"--pool-tx-hash and --amount-ckb are required"}' >&2
	exit 1
fi

curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{
		\"intent\": \"swap\",
		\"pool_tx_hash\": \"$POOL_TX_HASH\",
		\"pool_index\": $POOL_INDEX,
		\"amount_ckb\": $AMOUNT_CKB,
		\"slippage_bps\": $SLIPPAGE_BPS
	}"
