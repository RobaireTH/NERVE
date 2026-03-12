#!/usr/bin/env bash
# Open a Fiber payment channel with a peer.
#
# Usage: open_channel.sh --peer-lock-args 0x... --funding-ckb 100
# Output: JSON with channel_id on success.
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
PEER_LOCK_ARGS=""
FUNDING_CKB="100"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--peer-lock-args) PEER_LOCK_ARGS="$2"; shift 2 ;;
		--funding-ckb)    FUNDING_CKB="$2";    shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$PEER_LOCK_ARGS" ]]; then
	echo '{"error": "--peer-lock-args is required"}' >&2
	exit 1
fi

curl -sf -X POST "$MCP_URL/fiber/channels" \
	-H "Content-Type: application/json" \
	-d "{\"peer_lock_args\": \"$PEER_LOCK_ARGS\", \"funding_ckb\": $FUNDING_CKB}"
