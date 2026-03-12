#!/usr/bin/env bash
# Open a Fiber payment channel with a peer.
#
# Usage: open_channel.sh --peer-id <node_id> --funding-ckb 100 [--peer-address /ip4/...]
# Output: JSON with temporary_channel_id on success.
#
# The peer must be connected first. If --peer-address is given, connects automatically.
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
PEER_ID=""
PEER_ADDRESS=""
FUNDING_CKB="100"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--peer-id)       PEER_ID="$2";       shift 2 ;;
		--peer-address)  PEER_ADDRESS="$2";  shift 2 ;;
		--funding-ckb)   FUNDING_CKB="$2";   shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$PEER_ID" ]]; then
	echo '{"error": "--peer-id is required"}' >&2
	exit 1
fi

# Connect to peer if address provided.
if [[ -n "$PEER_ADDRESS" ]]; then
	curl -sf -X POST "$MCP_URL/fiber/peers" \
		-H "Content-Type: application/json" \
		-d "{\"peer_address\": \"$PEER_ADDRESS\"}" > /dev/null
	sleep 2
fi

curl -sf -X POST "$MCP_URL/fiber/channels" \
	-H "Content-Type: application/json" \
	-d "{\"peer_id\": \"$PEER_ID\", \"funding_ckb\": $FUNDING_CKB}"
