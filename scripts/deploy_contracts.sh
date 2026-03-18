#!/usr/bin/env bash
# Deploy NERVE type scripts to CKB testnet.
#
# Prerequisites:
#   - nerve-core server running with admin API enabled:
#       ENABLE_ADMIN_API=1 cargo run -p nerve-core
#   - AGENT_PRIVATE_KEY set in environment or .env
#   - Sufficient testnet CKB in the agent wallet
#
# Usage:
#   ./scripts/deploy_contracts.sh [agent_identity|job_cell|capability_nft|reputation|all]
#
# Output:
#   Appends/updates deployed addresses in .env.deployed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CONTRACTS_DIR="$ROOT_DIR/contracts"
CORE_URL="${CORE_URL:-http://localhost:8080}"
DEPLOYED_ENV="$ROOT_DIR/.env.deployed"
TARGET="riscv64imac-unknown-none-elf"
CONTRACT="${1:-all}"

touch "$DEPLOYED_ENV"

update_env() {
	local key="$1" val="$2"
	if grep -q "^${key}=" "$DEPLOYED_ENV" 2>/dev/null; then
		sed -i "s|^${key}=.*|${key}=${val}|" "$DEPLOYED_ENV"
	else
		echo "${key}=${val}" >> "$DEPLOYED_ENV"
	fi
}

deploy_binary() {
	local name="$1" env_code_hash="$2" env_dep_tx="$3"
	local binary="$CONTRACTS_DIR/target/$TARGET/release/$name"

	echo "==> Building $name (RISC-V)..."
	(cd "$CONTRACTS_DIR" && cargo build --target "$TARGET" --release --bin "$name" 2>&1)

	if [[ ! -f "$binary" ]]; then
		echo "ERROR: binary not found at $binary" >&2; exit 1
	fi

	local size; size=$(wc -c < "$binary")
	echo "    Size: $size bytes (~$(( (size + 61) )) CKB minimum capacity)"

	local hex; hex=$(xxd -p "$binary" | tr -d '\n')

	echo "==> Deploying $name via nerve-core ($CORE_URL)..."
	local response; response=$(curl -sf -X POST "$CORE_URL/admin/deploy-bin" \
		-H "Content-Type: application/json" \
		-d "{\"binary_hex\": \"$hex\"}")

	local tx_hash code_hash
	tx_hash=$(echo "$response" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
	code_hash=$(echo "$response" | grep -o '"code_hash":"[^"]*"' | cut -d'"' -f4)

	if [[ -z "$tx_hash" || -z "$code_hash" ]]; then
		echo "ERROR: unexpected response:" >&2; echo "$response" >&2; exit 1
	fi

	echo "    tx_hash:   $tx_hash"
	echo "    code_hash: $code_hash"

	update_env "$env_code_hash" "$code_hash"
	update_env "$env_dep_tx" "$tx_hash"

	echo "    Written to .env.deployed"
	echo ""
}

case "$CONTRACT" in
	agent_identity)
		deploy_binary "agent_identity" "AGENT_IDENTITY_TYPE_CODE_HASH" "AGENT_IDENTITY_DEP_TX_HASH"
		;;
	job_cell)
		deploy_binary "job_cell" "JOB_CELL_TYPE_CODE_HASH" "JOB_CELL_DEP_TX_HASH"
		;;
	capability_nft)
		deploy_binary "capability_nft" "CAP_NFT_TYPE_CODE_HASH" "CAP_NFT_DEP_TX_HASH"
		;;
	reputation)
		deploy_binary "reputation" "REPUTATION_TYPE_CODE_HASH" "REPUTATION_DEP_TX_HASH"
		;;
	all)
		deploy_binary "agent_identity" "AGENT_IDENTITY_TYPE_CODE_HASH" "AGENT_IDENTITY_DEP_TX_HASH"
		deploy_binary "job_cell"       "JOB_CELL_TYPE_CODE_HASH"       "JOB_CELL_DEP_TX_HASH"
		deploy_binary "capability_nft" "CAP_NFT_TYPE_CODE_HASH"        "CAP_NFT_DEP_TX_HASH"
		deploy_binary "reputation"     "REPUTATION_TYPE_CODE_HASH"     "REPUTATION_DEP_TX_HASH"
		;;
	*)
		echo "Usage: $0 [agent_identity|job_cell|capability_nft|reputation|all]" >&2; exit 1
		;;
esac

echo "==> Done. Source .env.deployed before running services:"
echo "    set -a && source .env.deployed && set +a"
