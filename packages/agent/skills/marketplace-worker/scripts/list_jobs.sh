#!/usr/bin/env bash
# List job cells from the MCP HTTP bridge.
#
# Usage: list_jobs.sh [--status Open|Reserved|Claimed] [--capability-hash 0x...]
# Output: JSON array of job cells.
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
if [[ -n "$STATUS" ]]; then
	QUERY="?status=$STATUS"
fi
if [[ -n "$CAPABILITY_HASH" ]]; then
	SEP="${QUERY:+&}${QUERY:-?}"
	QUERY="${QUERY}${SEP}capability_hash=$CAPABILITY_HASH"
fi

curl -sf "$MCP_URL/jobs${QUERY}"
