#!/usr/bin/env bash
# start_nerve.sh - Start all NERVE services with correct env

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="$ROOT/.env.deployed"
CORE_BIN="$ROOT/target/debug/nerve-core"
MCP_BIN="$ROOT/packages/mcp/dist/index.js"
LOG_DIR="/tmp"

POSTER_KEY="f9160112fbf2064619afc2c63817718e1ac3c4d6d5bff671103650221b249bda"
WORKER_KEY="c78d7af6f85af378cb3291cb81486993b68681043bde7f1dca7c50d5ff5abaa2"

POSTER_PORT=8080
WORKER_PORT=8090
MCP_PORT=8081

stop_all() {
  echo "Stopping NERVE services..."
  fuser -k ${POSTER_PORT}/tcp 2>/dev/null && echo "  poster stopped" || true
  fuser -k ${WORKER_PORT}/tcp 2>/dev/null && echo "  worker stopped" || true
  fuser -k ${MCP_PORT}/tcp   2>/dev/null && echo "  mcp stopped"    || true
  echo "Done."
}

if [[ "${1:-}" == "--stop" ]]; then
  stop_all; exit 0
fi

if [[ "${1:-}" == "--restart" ]]; then
  stop_all; sleep 2
fi

if [[ ! -f "$ENV_FILE" ]]; then
  echo "ERROR: $ENV_FILE not found. Run deploy_contracts.sh first." >&2
  exit 1
fi

if [[ ! -f "$CORE_BIN" ]]; then
  echo "ERROR: nerve-core binary not found at $CORE_BIN" >&2
  echo "Run: cargo build -p nerve-core" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source <(grep -v '^#' "$ENV_FILE" | grep -v '^$')
set +a

echo "Loaded env from $ENV_FILE"
echo "  JOB_CELL_TYPE_CODE_HASH = $JOB_CELL_TYPE_CODE_HASH"
echo "  JOB_CELL_DEP_TX_HASH    = $JOB_CELL_DEP_TX_HASH"
echo ""

echo "Starting poster (port $POSTER_PORT)..."
env \
  AGENT_IDENTITY_TYPE_CODE_HASH="$AGENT_IDENTITY_TYPE_CODE_HASH" \
  AGENT_IDENTITY_DEP_TX_HASH="$AGENT_IDENTITY_DEP_TX_HASH" \
  JOB_CELL_TYPE_CODE_HASH="$JOB_CELL_TYPE_CODE_HASH" \
  JOB_CELL_DEP_TX_HASH="$JOB_CELL_DEP_TX_HASH" \
  CAP_NFT_TYPE_CODE_HASH="$CAP_NFT_TYPE_CODE_HASH" \
  CAP_NFT_DEP_TX_HASH="$CAP_NFT_DEP_TX_HASH" \
  REPUTATION_TYPE_CODE_HASH="$REPUTATION_TYPE_CODE_HASH" \
  REPUTATION_DEP_TX_HASH="$REPUTATION_DEP_TX_HASH" \
  DOB_BADGE_CODE_HASH="$DOB_BADGE_CODE_HASH" \
  DOB_BADGE_DEP_TX_HASH="$DOB_BADGE_DEP_TX_HASH" \
  MOCK_AMM_TYPE_CODE_HASH="${MOCK_AMM_TYPE_CODE_HASH:-}" \
  MOCK_AMM_DEP_TX_HASH="${MOCK_AMM_DEP_TX_HASH:-}" \
  AGENT_PRIVATE_KEY="$POSTER_KEY" \
  CORE_PORT="$POSTER_PORT" \
  "$CORE_BIN" &>"$LOG_DIR/core-poster.log" &

echo "Starting worker (port $WORKER_PORT)..."
env \
  AGENT_IDENTITY_TYPE_CODE_HASH="$AGENT_IDENTITY_TYPE_CODE_HASH" \
  AGENT_IDENTITY_DEP_TX_HASH="$AGENT_IDENTITY_DEP_TX_HASH" \
  JOB_CELL_TYPE_CODE_HASH="$JOB_CELL_TYPE_CODE_HASH" \
  JOB_CELL_DEP_TX_HASH="$JOB_CELL_DEP_TX_HASH" \
  CAP_NFT_TYPE_CODE_HASH="$CAP_NFT_TYPE_CODE_HASH" \
  CAP_NFT_DEP_TX_HASH="$CAP_NFT_DEP_TX_HASH" \
  REPUTATION_TYPE_CODE_HASH="$REPUTATION_TYPE_CODE_HASH" \
  REPUTATION_DEP_TX_HASH="$REPUTATION_DEP_TX_HASH" \
  DOB_BADGE_CODE_HASH="$DOB_BADGE_CODE_HASH" \
  DOB_BADGE_DEP_TX_HASH="$DOB_BADGE_DEP_TX_HASH" \
  MOCK_AMM_TYPE_CODE_HASH="${MOCK_AMM_TYPE_CODE_HASH:-}" \
  MOCK_AMM_DEP_TX_HASH="${MOCK_AMM_DEP_TX_HASH:-}" \
  AGENT_PRIVATE_KEY="$WORKER_KEY" \
  CORE_PORT="$WORKER_PORT" \
  "$CORE_BIN" &>"$LOG_DIR/core-worker.log" &

if [[ -f "$MCP_BIN" ]]; then
  echo "Starting MCP bridge (port $MCP_PORT)..."
  node "$MCP_BIN" &>"$LOG_DIR/mcp.log" &
else
  echo "MCP binary not found at $MCP_BIN - skipping"
fi

echo ""
echo "Waiting for services..."
sleep 6

OK=true
for PORT_NAME in "$POSTER_PORT:poster" "$WORKER_PORT:worker"; do
  PORT="${PORT_NAME%%:*}"
  NAME="${PORT_NAME##*:}"
  BAL=$(curl -s "http://localhost:$PORT/agent/balance" \
    | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'{d[\"balance_ckb\"]:.2f} CKB')" 2>/dev/null || echo "not responding")
  if [[ "$BAL" == "not responding" ]]; then
    echo "  FAIL $NAME (port $PORT): $BAL"
    OK=false
  else
    echo "  OK   $NAME (port $PORT): $BAL"
  fi
done

MCP_STATUS=$(curl -s "http://localhost:$MCP_PORT/" \
  | python3 -c "import sys,json; json.load(sys.stdin); print('ok')" 2>/dev/null || echo "not responding")
if [[ "$MCP_STATUS" == "ok" ]]; then
  echo "  OK   mcp (port $MCP_PORT): up"
else
  echo "  FAIL mcp (port $MCP_PORT): $MCP_STATUS"
  OK=false
fi

echo ""
if $OK; then
  echo "All services up."
  echo "Logs: $LOG_DIR/core-poster.log, $LOG_DIR/core-worker.log, $LOG_DIR/mcp.log"
else
  echo "One or more services failed to start. Check logs in $LOG_DIR/"
  exit 1
fi
