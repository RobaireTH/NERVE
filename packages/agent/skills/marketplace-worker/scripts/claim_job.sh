#!/usr/bin/env bash
# Claim a Reserved job cell (transitions Reserved → Claimed).
#
# Usage: claim_job.sh --job-tx-hash 0x... --job-index 0
# Output: JSON with tx_hash on success, error on failure.
#
# Environment:
#   CORE_URL (default: http://localhost:8080)

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
JOB_TX_HASH=""
JOB_INDEX="0"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--job-tx-hash) JOB_TX_HASH="$2"; shift 2 ;;
		--job-index)   JOB_INDEX="$2";   shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$JOB_TX_HASH" ]]; then
	echo '{"error": "--job-tx-hash is required"}' >&2
	exit 1
fi

RESPONSE=$(curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{
		\"intent\": \"claim_job\",
		\"job_tx_hash\": \"$JOB_TX_HASH\",
		\"job_index\": $JOB_INDEX
	}")

echo "$RESPONSE"
