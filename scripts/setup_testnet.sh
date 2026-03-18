#!/usr/bin/env bash
# setup_testnet.sh — One-shot testnet setup for NERVE demo.
#
# Deploys all contracts, spawns agent identities,
# and creates reputation cells for both poster and worker agents.
#
# Prerequisites:
#   - nerve-core running on :8080 (poster) and :8090 (worker) with ENABLE_ADMIN_API=1
#   - DEMO_POSTER_KEY and DEMO_WORKER_KEY set in environment
#   - Sufficient testnet CKB in both wallets
#
# Usage:
#   source .env && ./scripts/setup_testnet.sh
#
# Output:
#   Writes all deployed addresses and cell outpoints to .env.deployed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
POSTER_URL="${POSTER_URL:-http://localhost:8080}"
WORKER_URL="${WORKER_URL:-http://localhost:8090}"

step()    { echo; echo "── $* ──"; }
ok()      { echo "   ✓ $*"; }
fail()    { echo "   ✗ $*" >&2; exit 1; }

post_tx() {
	local url="$1" body="$2"
	local resp; resp=$(curl -sf -X POST "$url/tx/build-and-broadcast" \
		-H "Content-Type: application/json" \
		-d "$body")
	local tx_hash; tx_hash=$(echo "$resp" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
	if [[ -z "$tx_hash" ]]; then
		echo "   FAILED: $resp" >&2
		return 1
	fi
	echo "$tx_hash"
}

wait_tx() {
	echo "   Waiting 15s for indexer..."
	sleep 15
}

# ── Step 1: Deploy contracts ─────────────────────────────────────────────────

step "Step 1: Deploying all contracts"
"$SCRIPT_DIR/deploy_contracts.sh" all
source "$ROOT_DIR/.env.deployed"

# ── Step 2: Spawn poster agent identity ──────────────────────────────────────

step "Step 2: Spawning poster agent identity (limit=20 CKB/tx, daily=200 CKB)"
POSTER_SPAWN_TX=$(post_tx "$POSTER_URL" \
	'{"intent":"spawn_agent","spending_limit_ckb":20,"daily_limit_ckb":200}') \
	|| fail "poster spawn_agent failed"
ok "Poster identity: $POSTER_SPAWN_TX:0"
wait_tx

# ── Step 3: Spawn worker agent identity ──────────────────────────────────────

step "Step 3: Spawning worker agent identity (limit=20 CKB/tx, daily=200 CKB)"
WORKER_SPAWN_TX=$(post_tx "$WORKER_URL" \
	'{"intent":"spawn_agent","spending_limit_ckb":20,"daily_limit_ckb":200}') \
	|| fail "worker spawn_agent failed"
ok "Worker identity: $WORKER_SPAWN_TX:0"
wait_tx

# ── Step 4: Create reputation cells ──────────────────────────────────────────

step "Step 4: Creating poster reputation cell"
POSTER_REP_TX=$(post_tx "$POSTER_URL" '{"intent":"create_reputation"}') \
	|| fail "poster create_reputation failed"
ok "Poster reputation: $POSTER_REP_TX:0"
wait_tx

step "Step 5: Creating worker reputation cell"
WORKER_REP_TX=$(post_tx "$WORKER_URL" '{"intent":"create_reputation"}') \
	|| fail "worker create_reputation failed"
ok "Worker reputation: $WORKER_REP_TX:0"
wait_tx

# ── Step 5.5: Spawn sub-agent under poster ───────────────────────────────────

step "Step 5.5: Spawning sub-agent under poster (10% revenue share)"
SUB_AGENT_TX=$(post_tx "$POSTER_URL" \
	'{"intent":"spawn_sub_agent","spending_limit_ckb":10,"daily_limit_ckb":100,"revenue_share_bps":1000}') \
	|| fail "poster spawn_sub_agent failed"
ok "Sub-agent identity: $SUB_AGENT_TX:0"
wait_tx

# ── Step 6: Mint capability NFT for worker ───────────────────────────────────

step "Step 7: Minting capability NFT for worker (text_summarize)"
# blake2b("text_summarize") — hardcoded for demo.
TEXT_SUMMARIZE_HASH="0x$(echo -n 'text_summarize' | xxd -p | tr -d '\n' | \
	python3 -c "
import sys, hashlib
data = bytes.fromhex(sys.stdin.read())
from hashlib import blake2b
h = blake2b(data, digest_size=32, person=b'ckb-default-hash')
print(h.hexdigest())
" 2>/dev/null || echo "0000000000000000000000000000000000000000000000000000000000000001")"

CAP_TX=$(post_tx "$WORKER_URL" \
	"{\"intent\":\"mint_capability\",\"capability_hash\":\"$TEXT_SUMMARIZE_HASH\"}") \
	|| fail "mint_capability failed"
ok "Capability NFT: $CAP_TX:0"

# ── Step 8: Fiber Network via fiber-pay (optional) ──────────────────────────

FIBER_STATUS="skipped"

step "Step 8: Fiber Network setup (optional)"

if [[ -n "${SKIP_FIBER:-}" ]]; then
	echo "   Skipped (SKIP_FIBER is set)."
elif [[ -n "${FIBER_RPC_URL:-}" ]]; then
	# User provided an external Fiber node — validate it responds.
	if curl -sf -X POST "${FIBER_RPC_URL}" \
		-H "Content-Type: application/json" \
		-d '{"jsonrpc":"2.0","id":1,"method":"node_info","params":[]}' \
		>/dev/null 2>&1; then
		ok "External Fiber node reachable at ${FIBER_RPC_URL}"
		FIBER_STATUS="external"
	else
		echo "   WARNING: FIBER_RPC_URL set but node not reachable at ${FIBER_RPC_URL}"
		echo "   Fiber features will be unavailable until the node is running."
		FIBER_STATUS="unreachable"
	fi
elif command -v npx >/dev/null 2>&1 && npx @fiber-pay/cli --version >/dev/null 2>&1; then
	# fiber-pay CLI is available — start a daemon node.
	echo "   Starting Fiber node via fiber-pay CLI..."
	if npx @fiber-pay/cli node start --daemon --network testnet --json 2>/dev/null; then
		ok "Fiber daemon started."
		echo "   Waiting for node readiness..."
		# Poll until the node reports ready (up to 60 seconds).
		for i in $(seq 1 12); do
			if npx @fiber-pay/cli node ready --json 2>/dev/null; then
				ok "Fiber node ready."
				FIBER_STATUS="fiber-pay"
				break
			fi
			sleep 5
		done
		if [[ "$FIBER_STATUS" != "fiber-pay" ]]; then
			echo "   WARNING: Fiber daemon started but not ready after 60s."
			echo "   Check status with: npx @fiber-pay/cli node status --json"
			FIBER_STATUS="fiber-pay-starting"
		fi
	else
		echo "   WARNING: Failed to start Fiber daemon via fiber-pay."
		echo "   Fiber features will be unavailable. To retry:"
		echo "     npx @fiber-pay/cli node start --daemon --network testnet --json"
		FIBER_STATUS="fiber-pay-failed"
	fi
else
	echo "   fiber-pay CLI not found and FIBER_RPC_URL not set."
	echo "   Fiber payment features will be unavailable."
	echo "   To enable Fiber, either:"
	echo "     1. Install fiber-pay: npm install -g @fiber-pay/cli"
	echo "     2. Set FIBER_RPC_URL to an existing Fiber node."
	FIBER_STATUS="unavailable"
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "NERVE Testnet Setup Complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Contracts:       .env.deployed"
echo "  Poster identity: $POSTER_SPAWN_TX:0"
echo "  Worker identity: $WORKER_SPAWN_TX:0"
echo "  Poster rep:      $POSTER_REP_TX:0"
echo "  Worker rep:      $WORKER_REP_TX:0"
echo "  Sub-agent:       $SUB_AGENT_TX:0 (10% rev share)"
echo "  Capability NFT:  $CAP_TX:0"
echo "  Fiber Network:   $FIBER_STATUS"
echo
echo "  Run: nerve demo"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
