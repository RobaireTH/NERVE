#!/usr/bin/env bash
# Cooperatively close a Fiber payment channel.
#
# Usage: close_channel.sh --channel-id <id>
# Output: JSON with close result.
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
CHANNEL_ID=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--channel-id) CHANNEL_ID="$2"; shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

if [[ -z "$CHANNEL_ID" ]]; then
	echo '{"error": "--channel-id is required"}' >&2
	exit 1
fi

curl -sf -X DELETE "$MCP_URL/fiber/channels/$CHANNEL_ID"
