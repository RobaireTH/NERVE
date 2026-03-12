#!/usr/bin/env bash
# setup_testnet.sh — One-shot testnet setup for NERVE demo.
#
# Deploys all contracts, spawns agent identities, creates a mock AMM pool,
# and creates reputation cells for both poster and worker agents.
#
# Prerequisites:
#   - nerve-core running on :8080 (poster) and :8090 (worker)
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

# ── Step 5: Create mock AMM pool ─────────────────────────────────────────────

step "Step 6: Creating mock AMM pool (1000 CKB / 1000000 tokens)"
POOL_TX=$(post_tx "$POSTER_URL" \
	'{"intent":"create_pool","seed_ckb":1000,"seed_token_amount":1000000}') \
	|| fail "create_pool failed"
ok "AMM pool: $POOL_TX:0"
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
echo "  AMM pool:        $POOL_TX:0"
echo "  Capability NFT:  $CAP_TX:0"
echo
echo "  Run: nerve demo"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
