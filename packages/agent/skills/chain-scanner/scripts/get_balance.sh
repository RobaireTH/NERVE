#!/usr/bin/env bash
# Get agent wallet balance from the TX Builder.
#
# Usage: get_balance.sh
# Output: JSON with balance_ckb and lock_args.
#
# Environment:
#   CORE_URL (default: http://localhost:8080)

set -euo pipefail

CORE_URL="${CORE_URL:-http://localhost:8080}"

curl -sf "$CORE_URL/agent/balance"
