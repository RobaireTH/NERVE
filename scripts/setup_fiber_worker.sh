#!/usr/bin/env bash
# Start a dedicated worker Fiber node for local/demo payments.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="$ROOT_DIR/fiber-worker-data"
RPC_PORT="${FIBER_WORKER_RPC_PORT:-9227}"
P2P_PORT="${FIBER_WORKER_P2P_PORT:-9228}"
CKB_RPC="${FIBER_CKB_RPC:-https://testnet.ckb.dev/rpc}"
FNN_BIN="${FNN_BIN:-fnn}"
KEY_PASSWORD="${FIBER_SECRET_KEY_PASSWORD:-nerve-dev}"
WORKER_KEY="${DEMO_WORKER_KEY:-${AGENT_PRIVATE_KEY:-}}"
ANNOUNCE_LISTENING_ADDR="${FIBER_WORKER_ANNOUNCE_LISTENING_ADDR:-false}"
ANNOUNCE_PRIVATE_ADDR="${FIBER_WORKER_ANNOUNCE_PRIVATE_ADDR:-true}"
PUBLIC_IP="${FIBER_WORKER_PUBLIC_IP:-}"
ANNOUNCED_NODE_NAME="${FIBER_WORKER_NODE_NAME:-nerve-worker}"

step() { echo; echo "-- $* --"; }
ok() { echo "   OK: $*"; }
fail() { echo "   FAIL: $*" >&2; exit 1; }

command -v "$FNN_BIN" >/dev/null 2>&1 || fail "fnn binary not found"
[[ -n "$WORKER_KEY" ]] || fail "DEMO_WORKER_KEY or AGENT_PRIVATE_KEY must be set"

mkdir -p "$DATA_DIR/ckb" "$DATA_DIR/fiber"

step "Writing worker Fiber config"
PUBLIC_ADDR_BLOCK=""
if [[ -n "$PUBLIC_IP" ]]; then
  PUBLIC_ADDR_BLOCK=$(cat <<EOF
  announced_addrs:
    - "/ip4/${PUBLIC_IP}/tcp/${P2P_PORT}"
EOF
)
fi
cat > "$DATA_DIR/config.yml" <<YAML
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
ok "Config written"

printf '%s' "${WORKER_KEY#0x}" > "$DATA_DIR/ckb/key"
chmod 600 "$DATA_DIR/ckb/key"

pkill -f "fnn.*fiber-worker-data" 2>/dev/null || true

step "Starting worker Fiber node"
FIBER_SECRET_KEY_PASSWORD="$KEY_PASSWORD" RUST_LOG=info nohup "$FNN_BIN" -d "$DATA_DIR" -c "$DATA_DIR/config.yml" > "$DATA_DIR/fnn.log" 2>&1 &

for i in $(seq 1 20); do
  if curl -sf -X POST "http://127.0.0.1:${RPC_PORT}" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"node_info","params":[]}' >/dev/null 2>&1; then
    break
  fi
  sleep 3
done

NODE_INFO=$(curl -sf -X POST "http://127.0.0.1:${RPC_PORT}" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"node_info","params":[]}')
NODE_ID=$(echo "$NODE_INFO" | grep -o '"node_id":"[^"]*"' | cut -d'"' -f4 || true)
[[ -n "$NODE_ID" ]] || fail "Could not read worker node id"

cat > "$ROOT_DIR/.env.fiber-worker" <<ENV
FIBER_WORKER_RPC_URL=http://127.0.0.1:${RPC_PORT}
FIBER_WORKER_P2P_PORT=${P2P_PORT}
FIBER_WORKER_NODE_ID=${NODE_ID}
FIBER_WORKER_PUBLIC_IP=${PUBLIC_IP}
FIBER_WORKER_ANNOUNCE_LISTENING_ADDR=${ANNOUNCE_LISTENING_ADDR}
FIBER_WORKER_ANNOUNCE_PRIVATE_ADDR=${ANNOUNCE_PRIVATE_ADDR}
ENV

ok "Worker Fiber RPC: http://127.0.0.1:${RPC_PORT}"
ok "Worker Fiber node id: ${NODE_ID}"
ok "Written .env.fiber-worker"
