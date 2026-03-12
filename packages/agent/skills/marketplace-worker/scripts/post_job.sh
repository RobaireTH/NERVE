#!/usr/bin/env bash
# Post a new job cell on CKB.
#
# Usage: post_job.sh --reward-ckb 5 --ttl-blocks 200 --capability-hash 0x000...
# Output: JSON with tx_hash on success, error on failure.
#
# Environment:
#   CORE_URL (default: http://localhost:8080)

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
REWARD_CKB="5"
TTL_BLOCKS="200"
CAPABILITY_HASH="0x0000000000000000000000000000000000000000000000000000000000000000"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--reward-ckb)     REWARD_CKB="$2";     shift 2 ;;
		--ttl-blocks)     TTL_BLOCKS="$2";     shift 2 ;;
		--capability-hash) CAPABILITY_HASH="$2"; shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

RESPONSE=$(curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{
		\"intent\": \"post_job\",
		\"reward_ckb\": $REWARD_CKB,
		\"ttl_blocks\": $TTL_BLOCKS,
		\"capability_hash\": \"$CAPABILITY_HASH\"
	}")

echo "$RESPONSE"
