#!/usr/bin/env bash
# test_integration.sh: Cold E2E integration test for all NERVE flows.
#
# Validates:
#   1. Identity: spawn agent identity
#   2. Marketplace: post → reserve → claim → complete
#   3. Capability NFT: mint with attestation proof
#   4. Reputation: create → propose → finalize
#   5. Badge: mint PoP badge for completed job
#   6. Sub-agent: spawn delegated identity

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && cd .. && pwd)" || ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

POSTER_PORT=8080
WORKER_PORT=8090
POSTER_URL="http://localhost:$POSTER_PORT"
WORKER_URL="http://localhost:$WORKER_PORT"

PASS=0
FAIL=0
SKIP=0

CKB_RPC="${CKB_RPC_URL:-https://testnet.ckb.dev/rpc}"

pass() { PASS=$((PASS + 1)); echo "   PASS: $*"; }
fail() { FAIL=$((FAIL + 1)); echo "   FAIL: $*" >&2; }
skip() { SKIP=$((SKIP + 1)); echo "   SKIP: $*"; }
section() { echo; echo "--- $* ---"; }

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
			echo "   $label tx committed (poll $i)"
			break
		fi
		echo "   … poll $i: $status, waiting 6s..."
		sleep 6
		[[ "$i" == "30" ]] && { fail "$label tx not committed after 30 polls"; return 1; }
	done
	echo "   … Waiting for indexer to pick up $label cell..."
	for i in $(seq 1 20); do
		local cell_status
		cell_status=$(curl -sf -X POST "$CKB_RPC" \
			-H "Content-Type: application/json" \
			-d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"get_live_cell\",\"params\":[{\"tx_hash\":\"$tx_hash\",\"index\":\"$out_index\"},false]}" \
			| grep -o '"status":"[^"]*"' | head -1 | cut -d'"' -f4)
		if [[ "$cell_status" == "live" ]]; then
			echo "   $label cell indexed (poll $i)"
			return 0
		fi
		echo "   … indexer poll $i: $cell_status, waiting 3s..."
		sleep 3
	done
	fail "$label cell not indexed after 60s"
	return 1
}
[[ -n "${DEMO_POSTER_KEY:-}" ]] || { echo "error: DEMO_POSTER_KEY not set" >&2; exit 1; }
[[ -n "${DEMO_WORKER_KEY:-}" ]] || { echo "error: DEMO_WORKER_KEY not set" >&2; exit 1; }
[[ -n "${JOB_CELL_TYPE_CODE_HASH:-}" ]] || { echo "error: JOB_CELL_TYPE_CODE_HASH not set" >&2; exit 1; }

echo "Starting poster nerve-core on :$POSTER_PORT"
AGENT_PRIVATE_KEY="$DEMO_POSTER_KEY" CORE_PORT="$POSTER_PORT" \
	cargo run -p nerve-core --quiet 2>/tmp/nerve-test-poster.log &
POSTER_PID=$!
sleep 3

echo "Starting worker nerve-core on :$WORKER_PORT"
AGENT_PRIVATE_KEY="$DEMO_WORKER_KEY" CORE_PORT="$WORKER_PORT" \
	cargo run -p nerve-core --quiet 2>/tmp/nerve-test-worker.log &
WORKER_PID=$!
sleep 3

cleanup() {
	kill "$POSTER_PID" "$WORKER_PID" 2>/dev/null || true
	echo
	echo "═══ RESULTS ═══"
	echo "  Passed:  $PASS"
	echo "  Failed:  $FAIL"
	echo "  Skipped: $SKIP"
	if [[ $FAIL -gt 0 ]]; then
		echo "  STATUS: FAILED"
		exit 1
	else
		echo "  STATUS: OK"
	fi
}
trap cleanup EXIT

section "Health Checks"

if curl -sf "$POSTER_URL/health" | grep -q '"status":"ok"'; then
	pass "poster health endpoint"
else
	fail "poster health endpoint"
fi

if curl -sf "$WORKER_URL/health" | grep -q '"status":"ok"'; then
	pass "worker health endpoint"
else
	fail "worker health endpoint"
fi

WORKER_BALANCE=$(curl -sf "$WORKER_URL/agent/balance")
WORKER_LOCK_ARGS=$(echo "$WORKER_BALANCE" | grep -o '"lock_args":"[^"]*"' | cut -d'"' -f4)
POSTER_BAL=$(curl -sf "$POSTER_URL/agent/balance" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)
WORKER_BAL=$(echo "$WORKER_BALANCE" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)

section "Balances"
echo "  Poster: $POSTER_BAL CKB"
echo "  Worker: $WORKER_BAL CKB"

if [[ -n "$WORKER_LOCK_ARGS" ]]; then
	pass "retrieved worker lock_args: $WORKER_LOCK_ARGS"
else
	fail "could not retrieve worker lock_args"
	exit 1
fi

# FLOW 0: Identity

section "FLOW 0: Identity Spawn"

if [[ -n "${AGENT_IDENTITY_TYPE_CODE_HASH:-}" ]]; then
	SPAWN_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d '{"intent":"spawn_agent","spending_limit_ckb":20,"daily_limit_ckb":200}') || true
	IDENTITY_TX=$(echo "$SPAWN_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

	if [[ -n "$IDENTITY_TX" && ${#IDENTITY_TX} -eq 66 ]]; then
		pass "spawn_agent → $IDENTITY_TX"
		wait_committed_and_indexed "$IDENTITY_TX" "0x0" "identity"
	else
		fail "spawn_agent returned: $SPAWN_RESP"
	fi

	SPAWN_W_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d '{"intent":"spawn_agent","spending_limit_ckb":20,"daily_limit_ckb":200}') || true
	W_IDENTITY_TX=$(echo "$SPAWN_W_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

	if [[ -n "$W_IDENTITY_TX" && ${#W_IDENTITY_TX} -eq 66 ]]; then
		pass "worker spawn_agent → $W_IDENTITY_TX"
		wait_committed_and_indexed "$W_IDENTITY_TX" "0x0" "worker identity"
	else
		fail "worker spawn_agent returned: $SPAWN_W_RESP"
	fi
else
	skip "AGENT_IDENTITY_TYPE_CODE_HASH not set"
fi

# FLOW 1: Agent Marketplace

section "FLOW 1: Agent Marketplace"

REWARD_CKB=62
TTL_BLOCKS=200
CAPABILITY="0x0000000000000000000000000000000000000000000000000000000000000000"

POST_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"post_job\",\"reward_ckb\":$REWARD_CKB,\"ttl_blocks\":$TTL_BLOCKS,\"capability_hash\":\"$CAPABILITY\"}") || true
JOB_TX_HASH=$(echo "$POST_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

if [[ -n "$JOB_TX_HASH" && ${#JOB_TX_HASH} -eq 66 ]]; then
	pass "post_job → $JOB_TX_HASH"
	wait_committed_and_indexed "$JOB_TX_HASH" "0x0" "job"
else
	fail "post_job returned: $POST_RESP"
fi

RESERVE_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"reserve_job\",\"job_tx_hash\":\"$JOB_TX_HASH\",\"job_index\":0,\"worker_lock_args\":\"$WORKER_LOCK_ARGS\"}") || true
RESERVE_TX=$(echo "$RESERVE_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

if [[ -n "$RESERVE_TX" && ${#RESERVE_TX} -eq 66 ]]; then
	pass "reserve_job → $RESERVE_TX"
	wait_committed_and_indexed "$RESERVE_TX" "0x0" "reserve"
else
	fail "reserve_job returned: $RESERVE_RESP"
fi

CLAIM_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"claim_job\",\"job_tx_hash\":\"$RESERVE_TX\",\"job_index\":0}") || true
CLAIM_TX=$(echo "$CLAIM_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

if [[ -n "$CLAIM_TX" && ${#CLAIM_TX} -eq 66 ]]; then
	pass "claim_job → $CLAIM_TX"
	wait_committed_and_indexed "$CLAIM_TX" "0x0" "claim"
else
	fail "claim_job returned: $CLAIM_RESP"
fi

COMPLETE_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"complete_job\",\"job_tx_hash\":\"$CLAIM_TX\",\"job_index\":0,\"worker_lock_args\":\"$WORKER_LOCK_ARGS\"}") || true
COMPLETE_TX=$(echo "$COMPLETE_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

if [[ -n "$COMPLETE_TX" && ${#COMPLETE_TX} -eq 66 ]]; then
	pass "complete_job → $COMPLETE_TX"
	wait_committed_and_indexed "$COMPLETE_TX" "0x0" "complete"
else
	fail "complete_job returned: $COMPLETE_RESP"
fi

NEW_WORKER_BAL=$(curl -sf "$WORKER_URL/agent/balance" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)
echo "  Worker balance after completion: $NEW_WORKER_BAL CKB"
pass "flow 1 finished (worker balance: $WORKER_BAL → $NEW_WORKER_BAL)"

section "FLOW 2: DeFi (UTXOSwap)"

skip "UTXOSwap DeFi tested via agent skills, not integration test"

section "FLOW 3: Capability NFT"

if [[ -n "${CAP_NFT_TYPE_CODE_HASH:-}" ]]; then
	DEMO_CAP_HASH="0x0000000000000000000000000000000000000000000000000000000000000001"
	CAP_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d "{\"intent\":\"mint_capability\",\"capability_hash\":\"$DEMO_CAP_HASH\"}") || true
	CAP_TX=$(echo "$CAP_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

	if [[ -n "$CAP_TX" && ${#CAP_TX} -eq 66 ]]; then
		pass "mint_capability → $CAP_TX"
		wait_committed_and_indexed "$CAP_TX" "0x0" "capability"
	else
		fail "mint_capability returned: $CAP_RESP"
	fi
else
	skip "CAP_NFT_TYPE_CODE_HASH not set"
fi

section "Reputation Lifecycle"

if [[ -n "${REPUTATION_TYPE_CODE_HASH:-}" ]]; then
	CREATE_REP_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d '{"intent":"create_reputation"}') || true
	REP_TX=$(echo "$CREATE_REP_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

	if [[ -n "$REP_TX" && ${#REP_TX} -eq 66 ]]; then
		pass "create_reputation → $REP_TX"
		wait_committed_and_indexed "$REP_TX" "0x0" "reputation"

		PROP_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
			-H "Content-Type: application/json" \
			-d "{\"intent\":\"propose_reputation\",\"rep_tx_hash\":\"$REP_TX\",\"rep_index\":0,\"propose_type\":1,\"dispute_window_blocks\":10}") || true
		PROP_TX=$(echo "$PROP_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

		if [[ -n "$PROP_TX" && ${#PROP_TX} -eq 66 ]]; then
			pass "propose_reputation → $PROP_TX"
			wait_committed_and_indexed "$PROP_TX" "0x0" "propose"

			echo "   … Waiting for dispute window (10 blocks ≈ 100s)..."
			sleep 110

			FIN_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
				-H "Content-Type: application/json" \
				-d "{\"intent\":\"finalize_reputation\",\"rep_tx_hash\":\"$PROP_TX\",\"rep_index\":0}") || true
			FIN_TX=$(echo "$FIN_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

			if [[ -n "$FIN_TX" && ${#FIN_TX} -eq 66 ]]; then
				pass "finalize_reputation → $FIN_TX"
			else
				fail "finalize_reputation returned: $FIN_RESP"
			fi
		else
			fail "propose_reputation returned: $PROP_RESP"
		fi
	else
		fail "create_reputation returned: $CREATE_REP_RESP"
	fi
else
	skip "REPUTATION_TYPE_CODE_HASH not set"
fi

section "Badge Mint"

if [[ -n "${DOB_BADGE_CODE_HASH:-}" && -n "${COMPLETE_TX:-}" ]]; then
	BADGE_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d "{\"intent\":\"mint_badge\",\"job_tx_hash\":\"$JOB_TX_HASH\",\"job_index\":0,\"worker_lock_args\":\"$WORKER_LOCK_ARGS\",\"completed_at_tx\":\"$COMPLETE_TX\"}") || true
	BADGE_TX=$(echo "$BADGE_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)

	if [[ -n "$BADGE_TX" && ${#BADGE_TX} -eq 66 ]]; then
		pass "mint_badge → $BADGE_TX"
	else
		fail "mint_badge returned: $BADGE_RESP"
	fi
else
	skip "DOB_BADGE_CODE_HASH not set or no completed job"
fi

section "Sub-Agent Delegation"

if [[ -n "${AGENT_IDENTITY_TYPE_CODE_HASH:-}" && -n "${IDENTITY_TX:-}" ]]; then
	SUBAGENT_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d '{"intent":"spawn_sub_agent","spending_limit_ckb":5,"daily_limit_ckb":50,"revenue_share_bps":1000,"initial_funding_ckb":100}') || true
	SUBAGENT_TX=$(echo "$SUBAGENT_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)
	SUBAGENT_LOCK=$(echo "$SUBAGENT_RESP" | grep -o '"sub_agent_lock_args":"[^"]*"' | cut -d'"' -f4 || true)

	if [[ -n "$SUBAGENT_TX" && ${#SUBAGENT_TX} -eq 66 ]]; then
		pass "spawn_sub_agent → $SUBAGENT_TX (lock: $SUBAGENT_LOCK)"
	else
		fail "spawn_sub_agent returned: $SUBAGENT_RESP"
	fi
else
	skip "No identity cell for sub-agent test"
fi

echo
echo "Integration test complete."
