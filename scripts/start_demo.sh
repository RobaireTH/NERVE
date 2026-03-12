#!/usr/bin/env bash
# start_demo.sh — Run the NERVE two-process marketplace demo.
#
# Starts two nerve-core instances (poster + worker) with separate keys
# and runs the full job lifecycle: post → reserve → claim → complete.
#
# Prerequisites:
#   - Contracts deployed: source .env.deployed
#   - DEMO_POSTER_KEY and DEMO_WORKER_KEY set in environment or .env
#   - CKB testnet reachable
#   - nerve-mcp bridge NOT required (uses TX Builder directly)
#
# Usage:
#   source .env && source .env.deployed && ./scripts/start_demo.sh
#   ./scripts/start_demo.sh --non-interactive   (skip confirmation prompts)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NON_INTERACTIVE="${1:-}"

POSTER_PORT=8080
WORKER_PORT=8090
POSTER_URL="http://localhost:$POSTER_PORT"
WORKER_URL="http://localhost:$WORKER_PORT"
MCP_URL="${MCP_URL:-http://localhost:8081}"

REWARD_CKB="${DEMO_REWARD_CKB:-5}"
TTL_BLOCKS="${DEMO_TTL_BLOCKS:-200}"

# ── Validation ─────────────────────────────────────────────────────────────────

[[ -n "${DEMO_POSTER_KEY:-}" ]] || { echo "error: DEMO_POSTER_KEY is not set" >&2; exit 1; }
[[ -n "${DEMO_WORKER_KEY:-}" ]] || { echo "error: DEMO_WORKER_KEY is not set" >&2; exit 1; }
[[ -n "${JOB_CELL_TYPE_CODE_HASH:-}" ]] || { echo "error: JOB_CELL_TYPE_CODE_HASH not set — run deploy_contracts.sh first" >&2; exit 1; }

step()    { echo; echo "── $* ──"; }
ok()      { echo "   ✓ $*"; }
pending() { echo "   … $*"; }
fail()    { echo "   ✗ $*" >&2; exit 1; }

# ── Start poster nerve-core ────────────────────────────────────────────────────

step "Starting poster nerve-core on :$POSTER_PORT"
AGENT_PRIVATE_KEY="$DEMO_POSTER_KEY" CORE_PORT="$POSTER_PORT" \
	cargo run -p nerve-core --quiet 2>/tmp/nerve-poster.log &
POSTER_PID=$!
echo "   PID: $POSTER_PID"
sleep 3

curl -sf "$POSTER_URL/health" | grep -q '"status":"ok"' || fail "poster nerve-core not healthy"
ok "Poster nerve-core running"

# ── Start worker nerve-core ────────────────────────────────────────────────────

step "Starting worker nerve-core on :$WORKER_PORT"
AGENT_PRIVATE_KEY="$DEMO_WORKER_KEY" CORE_PORT="$WORKER_PORT" \
	cargo run -p nerve-core --quiet 2>/tmp/nerve-worker.log &
WORKER_PID=$!
echo "   PID: $WORKER_PID"
sleep 3

curl -sf "$WORKER_URL/health" | grep -q '"status":"ok"' || fail "worker nerve-core not healthy"
ok "Worker nerve-core running"

cleanup() {
	kill "$POSTER_PID" "$WORKER_PID" 2>/dev/null || true
}
trap cleanup EXIT

# ── Fetch worker lock_args ─────────────────────────────────────────────────────

step "Fetching worker lock_args"
WORKER_BALANCE=$(curl -sf "$WORKER_URL/agent/balance")
WORKER_LOCK_ARGS=$(echo "$WORKER_BALANCE" | grep -o '"lock_args":"[^"]*"' | cut -d'"' -f4)
[[ -n "$WORKER_LOCK_ARGS" ]] || fail "could not read worker lock_args"
ok "Worker lock_args: $WORKER_LOCK_ARGS"

# ── Pre-flight balance check ───────────────────────────────────────────────────

step "Checking balances"
POSTER_BAL=$(curl -sf "$POSTER_URL/agent/balance" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)
WORKER_BAL=$(echo "$WORKER_BALANCE" | grep -o '"balance_ckb":[0-9.]*' | cut -d: -f2)
echo "   Poster: $POSTER_BAL CKB"
echo "   Worker: $WORKER_BAL CKB"

NEEDED=$(echo "$REWARD_CKB + 185 + 2" | bc)
echo "   Need (poster): ~$NEEDED CKB for job cell (184 overhead + $REWARD_CKB reward + fee)"

# ── Flow 1: Post Job ───────────────────────────────────────────────────────────

step "1. Poster: posting job ($REWARD_CKB CKB reward, TTL $TTL_BLOCKS blocks)"
CAPABILITY="0x0000000000000000000000000000000000000000000000000000000000000000"
POST_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"post_job\",\"reward_ckb\":$REWARD_CKB,\"ttl_blocks\":$TTL_BLOCKS,\"capability_hash\":\"$CAPABILITY\"}")
JOB_TX_HASH=$(echo "$POST_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$JOB_TX_HASH" ]] || fail "post_job failed: $POST_RESP"
ok "Job posted: $JOB_TX_HASH"
echo "   Explorer: https://testnet.explorer.nervos.org/transaction/$JOB_TX_HASH"

pending "Waiting 12s for job cell to be indexed..."
sleep 12

# ── Flow 2: Reserve ────────────────────────────────────────────────────────────

step "2. Worker: reserving job $JOB_TX_HASH:0"
RESERVE_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"reserve_job\",\"job_tx_hash\":\"$JOB_TX_HASH\",\"job_index\":0,\"worker_lock_args\":\"$WORKER_LOCK_ARGS\"}")
RESERVE_TX=$(echo "$RESERVE_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$RESERVE_TX" ]] || fail "reserve_job failed: $RESERVE_RESP"
ok "Job reserved: $RESERVE_TX"
echo "   Explorer: https://testnet.explorer.nervos.org/transaction/$RESERVE_TX"

pending "Waiting 12s..."
sleep 12

# ── Flow 3: Claim ──────────────────────────────────────────────────────────────

step "3. Worker: claiming job $RESERVE_TX:0"
CLAIM_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"claim_job\",\"job_tx_hash\":\"$RESERVE_TX\",\"job_index\":0}")
CLAIM_TX=$(echo "$CLAIM_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$CLAIM_TX" ]] || fail "claim_job failed: $CLAIM_RESP"
ok "Job claimed: $CLAIM_TX"
echo "   Explorer: https://testnet.explorer.nervos.org/transaction/$CLAIM_TX"

if [[ "$NON_INTERACTIVE" != "--non-interactive" ]]; then
	echo
	echo "   Worker is now executing the task (simulated)..."
	echo "   Press ENTER to complete the job and release the reward."
	read -r
fi

pending "Waiting 12s for claim to be indexed..."
sleep 12

# ── Flow 4: Complete ───────────────────────────────────────────────────────────

step "4. Poster: completing job $CLAIM_TX:0 (routes $REWARD_CKB CKB to worker)"
COMPLETE_RESP=$(curl -sf -X POST "$POSTER_URL/tx/build-and-broadcast" \
	-H "Content-Type: application/json" \
	-d "{\"intent\":\"complete_job\",\"job_tx_hash\":\"$CLAIM_TX\",\"job_index\":0,\"worker_lock_args\":\"$WORKER_LOCK_ARGS\"}")
COMPLETE_TX=$(echo "$COMPLETE_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
[[ -n "$COMPLETE_TX" ]] || fail "complete_job failed: $COMPLETE_RESP"
ok "Job completed: $COMPLETE_TX"
echo "   Explorer: https://testnet.explorer.nervos.org/transaction/$COMPLETE_TX"

# ── Summary ────────────────────────────────────────────────────────────────────

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "NERVE Demo — Marketplace Flow Complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  post:     $JOB_TX_HASH"
echo "  reserve:  $RESERVE_TX"
echo "  claim:    $CLAIM_TX"
echo "  complete: $COMPLETE_TX"
echo
echo "  Worker ($WORKER_LOCK_ARGS) received $REWARD_CKB CKB."
echo "  Reputation update: pending (dispute window active)."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
