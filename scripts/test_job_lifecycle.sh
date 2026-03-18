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

CKB_RPC="${CKB_RPC_URL:-https://testnet.ckb.dev/rpc}"

step() { echo; echo "── $* ──"; }
ok()   { echo "   OK: $*"; }
fail() { echo "   FAIL: $*" >&2; exit 1; }

# Wait for a TX to be committed, then wait for its output cell to be indexed.
wait_committed_and_indexed() {
	local tx_hash="$1" out_index="${2:-0x0}" label="${3:-cell}"
	echo "   … Waiting for $label tx to be committed..."
	for i in $(seq 1 30); do
		local status
		status=$(curl -sf -X POST "$CKB_RPC" \
			-H "Content-Type: application/json" \
			-d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"get_transaction\",\"params\":[\"$tx_hash\"]}" \
			| grep -o '"status":"[^"]*"' | head -1 | cut -d'"' -f4)
		if [[ "$status" == "committed" ]]; then
			ok "$label tx committed (poll $i)"
			break
		fi
		echo "   … poll $i: $status — waiting 6s..."
		sleep 6
		[[ "$i" == "30" ]] && fail "$label tx not committed after 30 polls"
	done
	echo "   … Waiting for indexer to pick up $label cell..."
	for i in $(seq 1 20); do
		local cell_status
		cell_status=$(curl -sf -X POST "$CKB_RPC" \
			-H "Content-Type: application/json" \
			-d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"get_live_cell\",\"params\":[{\"tx_hash\":\"$tx_hash\",\"index\":\"$out_index\"},false]}" \
			| grep -o '"status":"[^"]*"' | head -1 | cut -d'"' -f4)
		if [[ "$cell_status" == "live" ]]; then
			ok "$label cell indexed (poll $i)"
			return 0
		fi
		echo "   … indexer poll $i: $cell_status — waiting 3s..."
		sleep 3
	done
	fail "$label cell not indexed after 60s"
}

post_intent() {
	local body="$1"
	local response
	response=$(curl -sf -X POST "$CORE_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d "$body")
	echo "$response"
}

step "1. Post job (reward=62 CKB, TTL=100 blocks)"
POST_RESP=$(post_intent "{
  \"intent\": \"post_job\",
  \"reward_ckb\": 62,
  \"ttl_blocks\": 100,
  \"capability_hash\": \"$CAPABILITY_HASH\"
}")
JOB_TX_HASH=$(echo "$POST_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$JOB_TX_HASH" ]] || fail "post_job did not return tx_hash (response: $POST_RESP)"
ok "tx_hash=$JOB_TX_HASH"

wait_committed_and_indexed "$JOB_TX_HASH" "0x0" "job"

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

wait_committed_and_indexed "$RESERVE_TX" "0x0" "reserve"

step "3. Claim job"
CLAIM_RESP=$(post_intent "{
  \"intent\": \"claim_job\",
  \"job_tx_hash\": \"$RESERVE_TX\",
  \"job_index\": 0
}")
CLAIM_TX=$(echo "$CLAIM_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$CLAIM_TX" ]] || fail "claim_job failed (response: $CLAIM_RESP)"
ok "tx_hash=$CLAIM_TX"

wait_committed_and_indexed "$CLAIM_TX" "0x0" "claim"

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
