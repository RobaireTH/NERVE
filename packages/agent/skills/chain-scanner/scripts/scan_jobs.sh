#!/usr/bin/env bash
# Scan for job cells matching an optional status or capability filter.
#
# Usage: scan_jobs.sh [--status Open] [--capability-hash 0x...]
# Output: JSON with jobs array.
#
# Environment:
#   MCP_URL (default: http://localhost:8081)

set -euo pipefail

MCP_URL="${MCP_URL:-http://localhost:8081}"
STATUS=""
CAPABILITY_HASH=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--status)           STATUS="$2";           shift 2 ;;
		--capability-hash)  CAPABILITY_HASH="$2";  shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

QUERY=""
[[ -n "$STATUS" ]] && QUERY="?status=$STATUS"
if [[ -n "$CAPABILITY_HASH" ]]; then
	[[ -n "$QUERY" ]] && QUERY="${QUERY}&capability_hash=$CAPABILITY_HASH" || QUERY="?capability_hash=$CAPABILITY_HASH"
fi

curl -sf "$MCP_URL/jobs${QUERY}"
