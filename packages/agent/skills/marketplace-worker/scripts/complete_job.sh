#!/usr/bin/env bash
# Complete a Claimed job: destroys the job cell and routes reward to worker.
# If the job embeds Fiber payment metadata, this also triggers the MCP auto-payment flow.
#
# Usage: complete_job.sh --job-tx-hash 0x... --job-index 0 --worker-lock-args 0x... [--result "..."]
# Output: JSON with tx_hash on success, plus fiber_payment details when applicable.
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
JOB_TX_HASH=""
JOB_INDEX="0"
WORKER_LOCK_ARGS=""
RESULT=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--job-tx-hash)      JOB_TX_HASH="$2";      shift 2 ;;
		--job-index)        JOB_INDEX="$2";         shift 2 ;;
		--worker-lock-args) WORKER_LOCK_ARGS="$2";  shift 2 ;;
		--result)           RESULT="$2";            shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$JOB_TX_HASH" || -z "$WORKER_LOCK_ARGS" ]]; then
	echo '{"error": "--job-tx-hash and --worker-lock-args are required"}' >&2
	exit 1
fi

BODY=$(cat <<JSON
{"worker_lock_args":"$WORKER_LOCK_ARGS"$(if [[ -n "$RESULT" ]]; then printf ',"result":%s' "$(printf '%s' "$RESULT" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')"; fi)}
JSON
)

RESPONSE=$(curl -sf -X POST "$MCP_URL/jobs/$JOB_TX_HASH/$JOB_INDEX/complete" \
	-H "Content-Type: application/json" \
	-d "$BODY")

echo "$RESPONSE"
