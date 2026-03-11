#!/usr/bin/env bash
# Deploy NERVE type scripts to CKB testnet.
#
# Prerequisites:
#   - nerve-core server running: cargo run -p nerve-core
#   - AGENT_PRIVATE_KEY set in environment or .env
#   - Sufficient testnet CKB in the agent wallet (use: curl -X POST ... /dev/faucet or ckb-cli)
#
# Usage:
#   ./scripts/deploy_contracts.sh
#
# Output:
#   Writes AGENT_IDENTITY_TYPE_CODE_HASH and AGENT_IDENTITY_DEP_TX_HASH to .env.deployed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CONTRACTS_DIR="$ROOT_DIR/contracts"
CORE_URL="${CORE_URL:-http://localhost:8080}"
DEPLOYED_ENV="$ROOT_DIR/.env.deployed"

echo "==> Building agent_identity type script (RISC-V)..."
(cd "$CONTRACTS_DIR" && cargo build \
    --target riscv64imac-unknown-none-elf \
    --release \
    --bin agent_identity 2>&1)

BINARY="$CONTRACTS_DIR/target/riscv64imac-unknown-none-elf/release/agent_identity"
if [[ ! -f "$BINARY" ]]; then
    echo "ERROR: binary not found at $BINARY" >&2
    exit 1
fi

BINARY_SIZE=$(wc -c < "$BINARY")
echo "==> Binary size: $BINARY_SIZE bytes (requires ~$(( BINARY_SIZE / 100000000 + 1 )) CKB minimum)"
echo "    Full capacity needed: $(( (BINARY_SIZE + 61) )) CKB"

BINARY_HEX=$(xxd -p "$BINARY" | tr -d '\n')

echo "==> Deploying to CKB testnet via nerve-core ($CORE_URL)..."
RESPONSE=$(curl -sf -X POST "$CORE_URL/admin/deploy-bin" \
    -H "Content-Type: application/json" \
    -d "{\"binary_hex\": \"$BINARY_HEX\"}")

TX_HASH=$(echo "$RESPONSE" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
CODE_HASH=$(echo "$RESPONSE" | grep -o '"code_hash":"[^"]*"' | cut -d'"' -f4)

if [[ -z "$TX_HASH" || -z "$CODE_HASH" ]]; then
    echo "ERROR: unexpected response from nerve-core:" >&2
    echo "$RESPONSE" >&2
    exit 1
fi

echo "==> Deployed!"
echo "    tx_hash:   $TX_HASH"
echo "    code_hash: $CODE_HASH"

# Write / update .env.deployed.
touch "$DEPLOYED_ENV"

update_env() {
    local key="$1"
    local val="$2"
    if grep -q "^${key}=" "$DEPLOYED_ENV" 2>/dev/null; then
        sed -i "s|^${key}=.*|${key}=${val}|" "$DEPLOYED_ENV"
    else
        echo "${key}=${val}" >> "$DEPLOYED_ENV"
    fi
}

update_env "AGENT_IDENTITY_TYPE_CODE_HASH" "$CODE_HASH"
update_env "AGENT_IDENTITY_DEP_TX_HASH" "$TX_HASH"

echo ""
echo "==> Written to $DEPLOYED_ENV:"
echo "    AGENT_IDENTITY_TYPE_CODE_HASH=$CODE_HASH"
echo "    AGENT_IDENTITY_DEP_TX_HASH=$TX_HASH"
echo ""
echo "==> Source .env.deployed before running nerve-core:"
echo "    set -a && source .env.deployed && set +a"
