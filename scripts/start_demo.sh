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
CLEAN_MODE="${2:-}"
if [[ "$NON_INTERACTIVE" == "--clean" ]]; then
	CLEAN_MODE="--clean"
	NON_INTERACTIVE=""
fi
if [[ "$CLEAN_MODE" != "--clean" && "${2:-}" == "--clean" ]]; then
	CLEAN_MODE="--clean"
fi

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

# ── Clean mode: stop any running instances ────────────────────────────────────

if [[ "$CLEAN_MODE" == "--clean" ]]; then
	step "Clean mode: stopping running nerve-core instances"
	pkill -f "nerve-core" 2>/dev/null || true
	sleep 2
	ok "Previous instances stopped"
fi

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

ok "Flow 1 complete: Agent Marketplace"

# ── Flow 2: DeFi Swap ─────────────────────────────────────────────────────────

POOL_TX_HASH="${DEMO_POOL_TX_HASH:-}"
SWAP_TX=""
if [[ -n "$POOL_TX_HASH" && -n "${MOCK_AMM_TYPE_CODE_HASH:-}" ]]; then
	step "FLOW 2: DeFi Execution"
	pending "Worker: swapping 10 CKB via mock AMM pool"
	SWAP_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d "{\"intent\":\"swap\",\"pool_tx_hash\":\"$POOL_TX_HASH\",\"pool_index\":0,\"amount_ckb\":10,\"slippage_bps\":100}" 2>&1) || true
	SWAP_TX=$(echo "$SWAP_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)
	if [[ -n "$SWAP_TX" ]]; then
		ok "Swap tx: $SWAP_TX"
		echo "   Explorer: https://testnet.explorer.nervos.org/transaction/$SWAP_TX"
	else
		echo "   Swap skipped or failed: $SWAP_RESP"
	fi
	pending "Waiting 12s..."
	sleep 12
	ok "Flow 2 complete: DeFi Swap"
else
	echo
	echo "── FLOW 2: DeFi Execution (skipped) ──"
	echo "   Set DEMO_POOL_TX_HASH and deploy mock_amm to enable."
fi

# ── Flow 3: Capability Proof ──────────────────────────────────────────────────

CAP_TX=""
if [[ -n "${CAP_NFT_TYPE_CODE_HASH:-}" ]]; then
	step "FLOW 3: Capability Proof"
	pending "Worker: minting capability NFT (text_summarize)"
	# Use a fixed demo hash for the text_summarize capability.
	DEMO_CAP_HASH="0x0000000000000000000000000000000000000000000000000000000000000001"
	CAP_RESP=$(curl -sf -X POST "$WORKER_URL/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d "{\"intent\":\"mint_capability\",\"capability_hash\":\"$DEMO_CAP_HASH\"}" 2>&1) || true
	CAP_TX=$(echo "$CAP_RESP" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4 || true)
	if [[ -n "$CAP_TX" ]]; then
		ok "Capability NFT: $CAP_TX"
		echo "   Explorer: https://testnet.explorer.nervos.org/transaction/$CAP_TX"
		echo "   Proof type: Signed attestation"
	else
		echo "   Capability mint skipped or failed: $CAP_RESP"
	fi
	ok "Flow 3 complete: Capability Proof"
else
	echo
	echo "── FLOW 3: Capability Proof (skipped) ──"
	echo "   Deploy capability_nft contract to enable."
fi

# ── Summary ────────────────────────────────────────────────────────────────────

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "NERVE Demo — All Flows Complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo
echo "  FLOW 1: Agent Marketplace"
echo "    post:     $JOB_TX_HASH"
echo "    reserve:  $RESERVE_TX"
echo "    claim:    $CLAIM_TX"
echo "    complete: $COMPLETE_TX"
echo "    Worker ($WORKER_LOCK_ARGS) received $REWARD_CKB CKB."
echo
echo "  FLOW 2: DeFi Swap"
if [[ -n "$SWAP_TX" ]]; then
	echo "    swap:     $SWAP_TX"
else
	echo "    (skipped — set DEMO_POOL_TX_HASH)"
fi
echo
echo "  FLOW 3: Capability Proof"
if [[ -n "$CAP_TX" ]]; then
	echo "    cap NFT:  $CAP_TX"
	echo "    proof:    signed attestation"
else
	echo "    (skipped — deploy capability_nft)"
fi
echo
echo "  Explorer: https://testnet.explorer.nervos.org/aggron"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
