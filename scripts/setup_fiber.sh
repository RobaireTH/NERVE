#!/usr/bin/env bash
# Set up and start a local Fiber Network node for NERVE development.
#
# The Fiber node exposes:
#   RPC:  http://localhost:8227  (FIBER_RPC_URL)
#   P2P:  tcp/8228               (FIBER_P2P_PORT)
#
# Prerequisites:
#   - fnn binary in PATH (download from https://github.com/nervosnetwork/fiber/releases)
#   - AGENT_PRIVATE_KEY set (the same key as nerve-core, or a separate Fiber key)
#   - CKB testnet reachable
#
# Usage:
#   source .env && ./scripts/setup_fiber.sh [--reset]
#
# After running, source .env.fiber to export FIBER_RPC_URL, FIBER_NODE_ID.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DATA_DIR="$ROOT_DIR/fiber-data"
RPC_PORT="${FIBER_RPC_PORT:-8227}"
P2P_PORT="${FIBER_P2P_PORT:-8228}"
CKB_RPC="${FIBER_CKB_RPC:-https://testnet.ckb.dev/rpc}"
FNN_BIN="${FNN_BIN:-fnn}"
KEY_PASSWORD="${FIBER_SECRET_KEY_PASSWORD:-nerve-dev}"
ANNOUNCE_LISTENING_ADDR="${FIBER_ANNOUNCE_LISTENING_ADDR:-false}"
ANNOUNCE_PRIVATE_ADDR="${FIBER_ANNOUNCE_PRIVATE_ADDR:-true}"
PUBLIC_IP="${FIBER_PUBLIC_IP:-}"
ANNOUNCED_NODE_NAME="${FIBER_NODE_NAME:-nerve-node}"

RESET="${1:-}"

step() { echo; echo "── $* ──"; }
ok()   { echo "   OK: $*"; }
fail() { echo "   FAIL: $*" >&2; exit 1; }

# Preflight checks

command -v "$FNN_BIN" >/dev/null 2>&1 || fail "fnn binary not found. Install from https://github.com/nervosnetwork/fiber/releases"

# Reset

if [[ "$RESET" == "--reset" ]]; then
	step "Resetting Fiber data dir"
	rm -rf "$DATA_DIR"
	ok "Removed $DATA_DIR"
fi

mkdir -p "$DATA_DIR/ckb" "$DATA_DIR/fiber"

# Write config

step "Writing Fiber node config"
PUBLIC_ADDR_BLOCK=""
if [[ -n "$PUBLIC_IP" ]]; then
	PUBLIC_ADDR_BLOCK=$(cat <<EOF
  announced_addrs:
    - "/ip4/${PUBLIC_IP}/tcp/${P2P_PORT}"
EOF
)
fi
cat > "$DATA_DIR/config.yml" <<YAML
## Fiber Network node config for NERVE development (testnet).

fiber:
  listening_addr: "/ip4/0.0.0.0/tcp/${P2P_PORT}"
  bootnode_addrs:
    - "/ip4/54.179.226.154/tcp/8228/p2p/Qmes1EBD4yNo9Ywkfe6eRw9tG1nVNGLDmMud1xJMsoYFKy"
    - "/ip4/16.163.7.105/tcp/8228/p2p/QmdyQWjPtbK4NWWsvy8s69NGJaQULwgeQDT5ZpNDrTNaeV"
  announce_listening_addr: ${ANNOUNCE_LISTENING_ADDR}
  announced_node_name: "${ANNOUNCED_NODE_NAME}"
  announce_private_addr: ${ANNOUNCE_PRIVATE_ADDR}
${PUBLIC_ADDR_BLOCK}
  chain: testnet
  scripts:
    - name: FundingLock
      script:
        code_hash: 0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c
        hash_type: type
        args: 0x
      cell_deps:
        - type_id:
            code_hash: 0x00000000000000000000000000000000000000000000000000545950455f4944
            hash_type: type
            args: 0x3cb7c0304fe53f75bb5727e2484d0beae4bd99d979813c6fc97c3cca569f10f6
        - cell_dep:
            out_point:
              tx_hash: 0x12c569a258dd9c5bd99f632bb8314b1263b90921ba31496467580d6b79dd14a7
              index: 0x0
            dep_type: code
    - name: CommitmentLock
      script:
        code_hash: 0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8
        hash_type: type
        args: 0x
      cell_deps:
        - type_id:
            code_hash: 0x00000000000000000000000000000000000000000000000000545950455f4944
            hash_type: type
            args: 0xf7e458887495cf70dd30d1543cad47dc1dfe9d874177bf19291e4db478d5751b
        - cell_dep:
            out_point:
              tx_hash: 0x12c569a258dd9c5bd99f632bb8314b1263b90921ba31496467580d6b79dd14a7
              index: 0x0
            dep_type: code

rpc:
  listening_addr: "127.0.0.1:${RPC_PORT}"

ckb:
  rpc_url: "${CKB_RPC}"

services:
  - fiber
  - rpc
  - ckb
YAML
ok "Config written to $DATA_DIR/config.yml"

# Import key (if AGENT_PRIVATE_KEY is set)

if [[ -n "${AGENT_PRIVATE_KEY:-}" ]]; then
	# fnn reads a plaintext key from ckb/key on first start, then encrypts it
	# to ckb/secret_key using FIBER_SECRET_KEY_PASSWORD.
	if [[ ! -f "$DATA_DIR/ckb/secret_key" ]]; then
		KEY="${AGENT_PRIVATE_KEY#0x}"
		printf '%s' "$KEY" > "$DATA_DIR/ckb/key"
		chmod 600 "$DATA_DIR/ckb/key"
		ok "Private key written to $DATA_DIR/ckb/key (will be encrypted on first start)"
	else
		ok "Encrypted key already exists at $DATA_DIR/ckb/secret_key"
	fi
fi

# Stop existing instance

if pgrep -f "fnn.*fiber-data" >/dev/null 2>&1; then
	step "Stopping existing fnn process"
	pkill -f "fnn.*fiber-data" || true
	sleep 2
	ok "Stopped"
fi

# Start Fiber node

step "Starting Fiber node (fnn)"
FIBER_SECRET_KEY_PASSWORD="$KEY_PASSWORD" RUST_LOG=info \
	nohup "$FNN_BIN" \
		-d "$DATA_DIR" \
		-c "$DATA_DIR/config.yml" \
	> "$DATA_DIR/fnn.log" 2>&1 &
FNN_PID=$!
ok "fnn started with PID $FNN_PID"

# Wait for RPC

step "Waiting for Fiber RPC to be ready"
for i in $(seq 1 20); do
	if curl -sf -X POST "http://127.0.0.1:${RPC_PORT}" \
		-H "Content-Type: application/json" \
		-d '{"jsonrpc":"2.0","id":1,"method":"node_info","params":[]}' \
		>/dev/null 2>&1; then
		ok "Fiber RPC is up at http://127.0.0.1:${RPC_PORT}"
		break
	fi
	if ! kill -0 "$FNN_PID" 2>/dev/null; then
		echo "   fnn exited early. Check $DATA_DIR/fnn.log"
		tail -20 "$DATA_DIR/fnn.log" 2>/dev/null
		fail "fnn process died"
	fi
	echo "   Waiting... ($i/20)"
	sleep 3
done

# Fetch node info

step "Fetching node info"
NODE_INFO=$(curl -sf -X POST "http://127.0.0.1:${RPC_PORT}" \
	-H "Content-Type: application/json" \
	-d '{"jsonrpc":"2.0","id":1,"method":"node_info","params":[]}' 2>/dev/null || echo '{}')
NODE_ID=$(echo "$NODE_INFO" | grep -o '"node_id":"[^"]*"' | cut -d'"' -f4 || true)
PEER_COUNT=$(echo "$NODE_INFO" | grep -o '"peers_count":"[^"]*"' | cut -d'"' -f4 || true)

if [[ -n "$NODE_ID" ]]; then
	ok "Node ID: $NODE_ID"
	ok "Connected peers: ${PEER_COUNT:-0}"
	cat > "$ROOT_DIR/.env.fiber" <<ENV
FIBER_RPC_URL=http://127.0.0.1:${RPC_PORT}
FIBER_P2P_PORT=${P2P_PORT}
FIBER_NODE_ID=${NODE_ID}
FIBER_PUBLIC_IP=${PUBLIC_IP}
FIBER_ANNOUNCE_LISTENING_ADDR=${ANNOUNCE_LISTENING_ADDR}
FIBER_ANNOUNCE_PRIVATE_ADDR=${ANNOUNCE_PRIVATE_ADDR}
ENV
	ok "Written to .env.fiber. Source it to export Fiber env vars."
else
	echo "   Could not read node ID (node may still be starting)."
fi

echo
echo "==> Fiber node started."
echo "    RPC:  http://127.0.0.1:${RPC_PORT}"
echo "    P2P:  tcp/${P2P_PORT}"
echo "    Logs: $DATA_DIR/fnn.log"
