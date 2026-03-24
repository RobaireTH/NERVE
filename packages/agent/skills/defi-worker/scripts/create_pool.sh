#!/usr/bin/env bash

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"
SEED_CKB="${SEED_CKB:-1000}"
SEED_TOKENS="${SEED_TOKENS:-1000000}"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--seed-ckb)    SEED_CKB="$2"; shift 2 ;;
		--seed-tokens) SEED_TOKENS="$2"; shift 2 ;;
		*) echo "{\"error\": \"unknown argument: $1\"}" >&2; exit 1 ;;
	esac
done

curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{
		\"intent\": \"create_pool\",
		\"seed_ckb\": $SEED_CKB,
		\"seed_token_amount\": $SEED_TOKENS
	}"
