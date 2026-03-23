#!/usr/bin/env bash
# Pay an agent over Fiber by lock_args.
#
# Usage:
#   pay_agent.sh --lock-args 0x... --amount-ckb 5 [--description "payment"]
#
# Output:
#   JSON from POST /fiber/pay-agent
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
LOCK_ARGS=""
AMOUNT_CKB=""
DESCRIPTION=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--lock-args)   LOCK_ARGS="$2";   shift 2 ;;
		--amount-ckb)  AMOUNT_CKB="$2";  shift 2 ;;
		--description) DESCRIPTION="$2"; shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$LOCK_ARGS" || -z "$AMOUNT_CKB" ]]; then
	echo '{"error": "provide --lock-args and --amount-ckb"}' >&2
	exit 1
fi

BODY=$(cat <<JSON
{"lock_args":"$LOCK_ARGS","amount_ckb":$AMOUNT_CKB,"description":"$DESCRIPTION"}
JSON
)

curl -sf -X POST "$MCP_URL/fiber/pay-agent" \
	-H "Content-Type: application/json" \
	-d "$BODY"
