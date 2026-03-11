#!/usr/bin/env bash
# Smoke-test the full job lifecycle against a running nerve-core instance.
#
# Prerequisites:
#   - nerve-core running: cargo run -p nerve-core
#   - JOB_CELL_TYPE_CODE_HASH, JOB_CELL_DEP_TX_HASH set (from .env.deployed)
#   - Sufficient testnet CKB in the agent wallet
#
# Usage:
#   source .env.deployed && ./scripts/test_job_lifecycle.sh [CORE_URL]

set -euo pipefail

CORE_URL="${1:-${CORE_URL:-http://localhost:8080}}"
WORKER_LOCK_ARGS="${WORKER_LOCK_ARGS:-0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef}"

# A dummy 32-byte capability hash (all zeros for testing).
CAPABILITY_HASH="0x0000000000000000000000000000000000000000000000000000000000000000"

step() { echo; echo "── $* ──"; }
ok()   { echo "   OK: $*"; }
fail() { echo "   FAIL: $*" >&2; exit 1; }

post_intent() {
	local body="$1"
	local response
	response=$(curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d "$body")
	echo "$response"
}

step "1. Post job (reward=50 CKB, TTL=100 blocks)"
POST_RESP=$(post_intent "{
  \"intent\": \"post_job\",
  \"reward_ckb\": 50,
  \"ttl_blocks\": 100,
  \"capability_hash\": \"$CAPABILITY_HASH\"
}")
JOB_TX_HASH=$(echo "$POST_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$JOB_TX_HASH" ]] || fail "post_job did not return tx_hash (response: $POST_RESP)"
ok "tx_hash=$JOB_TX_HASH"

echo "   Waiting 5 s for tx to propagate..."
sleep 5

step "2. Reserve job (worker=$WORKER_LOCK_ARGS)"
RESERVE_RESP=$(post_intent "{
  \"intent\": \"reserve_job\",
  \"job_tx_hash\": \"$JOB_TX_HASH\",
  \"job_index\": 0,
  \"worker_lock_args\": \"$WORKER_LOCK_ARGS\"
}")
RESERVE_TX=$(echo "$RESERVE_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$RESERVE_TX" ]] || fail "reserve_job failed (response: $RESERVE_RESP)"
ok "tx_hash=$RESERVE_TX"

sleep 5

step "3. Claim job"
CLAIM_RESP=$(post_intent "{
  \"intent\": \"claim_job\",
  \"job_tx_hash\": \"$RESERVE_TX\",
  \"job_index\": 0
}")
CLAIM_TX=$(echo "$CLAIM_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$CLAIM_TX" ]] || fail "claim_job failed (response: $CLAIM_RESP)"
ok "tx_hash=$CLAIM_TX"

sleep 5

step "4. Complete job (routes reward to worker)"
COMPLETE_RESP=$(post_intent "{
  \"intent\": \"complete_job\",
  \"job_tx_hash\": \"$CLAIM_TX\",
  \"job_index\": 0,
  \"worker_lock_args\": \"$WORKER_LOCK_ARGS\"
}")
COMPLETE_TX=$(echo "$COMPLETE_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$COMPLETE_TX" ]] || fail "complete_job failed (response: $COMPLETE_RESP)"
ok "tx_hash=$COMPLETE_TX"

echo
echo "==> Full lifecycle PASSED."
echo "    post:     $JOB_TX_HASH"
echo "    reserve:  $RESERVE_TX"
echo "    claim:    $CLAIM_TX"
echo "    complete: $COMPLETE_TX"
