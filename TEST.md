# Integration Testing Guide for NERVE SupeRISE Integration

## Overview

This document guides testing of the SupeRISE optional signing backend integration alongside the existing LocalSigner mode. All tests verify that both modes produce identical on-chain transactions while maintaining backward compatibility.

## Test Environment Setup

### Prerequisites

- Rust stable with RISC-V target
- Node.js v20+
- CKB testnet access (RPC: `https://testnet.ckb.dev/rpc`)
- Testnet CKB from [faucet.nervos.org](https://faucet.nervos.org)

### Environment Variables

Create or update `.env`:

```bash
# Shared
CKB_RPC_URL=https://testnet.ckb.dev/rpc
CKB_INDEXER_URL=https://testnet.ckb.dev/indexer
CORE_URL=http://localhost:8080
CORE_PORT=8080
MCP_PORT=8081

# Signing backend configuration
# Mode 1: Local (default, existing behavior)
SIGNING_BACKEND=local
AGENT_PRIVATE_KEY=0x<your-32-byte-hex-key>

# Mode 2: SupeRISE (optional wallet backend)
# SIGNING_BACKEND=superise
# SUPERISE_URL=http://127.0.0.1:18799/mcp
# AGENT_PRIVATE_KEY is NOT required for SupeRISE mode

# Spending limits (enforced on-chain)
DAILY_SPEND_LIMIT_CKB=100
PER_TX_LIMIT_CKB=20

# Demo mode (optional, for full end-to-end testing)
DEMO_POSTER_KEY=0x<separate-key-for-poster>
DEMO_WORKER_KEY=0x<separate-key-for-worker>
```

## Test Categories

### 1. Unit Tests (Local Execution)

All unit tests are Rust-based and run without network access.

```bash
# Run all Rust tests in nerve-core
cd packages/core
cargo test

# Run specific test module
cargo test signer:: -v

# Run with output
cargo test -- --nocapture
```

**Expected results:**
- 58 tests pass
- Zero compiler warnings
- All signing module tests verify signature generation and witness construction

### 2. Component Tests: LocalSigner Mode

Test the default signing backend with local private key.

#### 2.1 Build and Start Services

```bash
# Terminal 1: Start nerve-core (LocalSigner mode)
source .env
cargo build --release -p nerve-core
./target/release/nerve-core

# Expected output: "Server running on http://localhost:8080"
```

```bash
# Terminal 2: Start nerve-mcp (HTTP bridge)
cd packages/mcp
npm install
npx tsx src/index.ts

# Expected output: "MCP bridge listening on port 8081"
```

#### 2.2 Test Basic Signing Flow

```bash
# Terminal 3: Test balance endpoint
curl http://localhost:8080/balance

# Expected response:
# {
#   "balance": "<amount>",
#   "lock_args": "0x<20-byte-hex>"
# }
```

#### 2.3 Test Transaction Building and Signing

```bash
# Test transfer flow (build + sign + broadcast)
curl -X POST http://localhost:8080/transfer \
  -H "Content-Type: application/json" \
  -d '{
    "to_address": "ckt1qzda...",
    "amount_ckb": 10
  }'

# Expected response:
# {
#   "tx_hash": "0x...",
#   "status": "sent"
# }
```

#### 2.4 Test Job Posting

```bash
# Post a job cell
curl -X POST http://localhost:8080/post-job \
  -H "Content-Type: application/json" \
  -d '{
    "reward_ckb": 5,
    "ttl_blocks": 100,
    "capability_required": "text_summarization"
  }'

# Expected response: job cell address and transaction hash
```

### 3. Component Tests: SuperiseSigner Mode (Optional)

Test the SupeRISE wallet backend integration.

#### 3.1 Start SupeRISE (if available)

```bash
# SupeRISE must be running separately
# For testing without SupeRISE installed, skip to integration tests

docker run -p 18799:18799 superise:latest
# or use local SupeRISE binary

# Verify SupeRISE is responding
curl http://localhost:18799/mcp \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": "1",
    "method": "nervos.address",
    "params": []
  }'

# Expected response: CKB address (ckt1... or ckb1...)
```

#### 3.2 Switch nerve-core to SupeRISE Mode

Edit `.env`:
```bash
SIGNING_BACKEND=superise
SUPERISE_URL=http://127.0.0.1:18799/mcp
# Do NOT include AGENT_PRIVATE_KEY for SupeRISE mode
```

Restart nerve-core:
```bash
cargo build --release -p nerve-core
./target/release/nerve-core

# Expected output: "Initialized SupeRISE signer from http://127.0.0.1:18799/mcp"
```

#### 3.3 Verify Lock Args Match

```bash
# Check lock_args endpoint returns the same value as SupeRISE address
curl http://localhost:8080/lock-args

# Should match the address returned by SupeRISE nervos.address call
```

#### 3.4 Test SupeRISE Signing

```bash
# Post a job using SupeRISE signing backend
# Should work identically to LocalSigner mode
curl -X POST http://localhost:8080/post-job \
  -H "Content-Type: application/json" \
  -d '{
    "reward_ckb": 5,
    "ttl_blocks": 100,
    "capability_required": "text_summarization"
  }'

# Expected: Transaction broadcasts correctly via SupeRISE signing
```

### 4. Integration Tests: End-to-End Flows

#### 4.1 Agent Marketplace Flow

**Scenario:** Two agents (poster + worker) interact in the marketplace.

```bash
# Terminal 1: Poster agent (DEMO_POSTER_KEY)
export AGENT_PRIVATE_KEY=$DEMO_POSTER_KEY
cargo run --release -p nerve-core -- --port 8080

# Terminal 2: Worker agent (DEMO_WORKER_KEY)
export AGENT_PRIVATE_KEY=$DEMO_WORKER_KEY
cargo run --release -p nerve-core -- --port 8090

# Terminal 3: Test flow
# 1. Poster posts a job
JOB_TX=$(curl -s -X POST http://localhost:8080/post-job \
  -H "Content-Type: application/json" \
  -d '{
    "reward_ckb": 5,
    "ttl_blocks": 100
  }' | jq -r '.tx_hash')

echo "Posted job with tx: $JOB_TX"

# Wait for confirmation
sleep 10

# 2. Worker detects and reserves the job
RESERVE_TX=$(curl -s -X POST http://localhost:8090/reserve-job \
  -H "Content-Type: application/json" \
  -d "{\"job_tx\": \"$JOB_TX\"}" | jq -r '.tx_hash')

echo "Reserved job with tx: $RESERVE_TX"

# 3. Worker claims the job
sleep 5
CLAIM_TX=$(curl -s -X POST http://localhost:8090/claim-job \
  -H "Content-Type: application/json" \
  -d "{\"job_tx\": \"$JOB_TX\"}" | jq -r '.tx_hash')

echo "Claimed job with tx: $CLAIM_TX"

# 4. Poster completes the job (releases reward)
sleep 5
SETTLE_TX=$(curl -s -X POST http://localhost:8080/complete-job \
  -H "Content-Type: application/json" \
  -d "{\"job_tx\": \"$JOB_TX\"}" | jq -r '.tx_hash')

echo "Completed job with tx: $SETTLE_TX"

# 5. Verify worker received reward
WORKER_BALANCE=$(curl -s http://localhost:8090/balance | jq '.balance')
echo "Worker final balance: $WORKER_BALANCE"
```

**Expected results:**
- All 4 transactions (post, reserve, claim, complete) confirm on testnet
- Worker lock_args correctly encoded in job cell
- Job cell state transitions: Open → Reserved → Claimed → Completed
- Reward (5 CKB minus fees) received by worker

#### 4.2 Capability Proof Flow

**Scenario:** Agent mints and proves a capability.

```bash
# 1. Mint capability NFT
CAPABILITY_TX=$(curl -s -X POST http://localhost:8080/mint-capability \
  -H "Content-Type: application/json" \
  -d '{
    "capability_hash": "0x'"$(openssl rand -hex 32)"'",
    "description": "text_summarization"
  }' | jq -r '.tx_hash')

echo "Minted capability with tx: $CAPABILITY_TX"

# 2. Verify capability cell data contains signed attestation
sleep 5
curl -s http://localhost:8080/get-capability?hash=$CAPABILITY_HASH | jq '.attestation_proof'

# Expected: 65-byte recoverable signature encoded in cell data
```

**Expected results:**
- Capability cell deployed on-chain
- Cell contains: agent_lock_args, capability_hash, signed attestation
- Attestation is 65 bytes (64-byte signature + 1-byte recovery ID)

#### 4.3 Reputation Update Flow

**Scenario:** Job completion triggers reputation update with dispute window.

```bash
# After job completion (from integration test 4.1), propose reputation
REP_TX=$(curl -s -X POST http://localhost:8080/propose-reputation \
  -H "Content-Type: application/json" \
  -d "{
    \"worker_lock_args\": \"0x...\",
    \"job_tx\": \"$JOB_TX\",
    \"completed\": true,
    \"dispute_blocks\": 10
  }" | jq -r '.tx_hash')

echo "Proposed reputation with tx: $REP_TX"

# Wait for dispute window to close (10 blocks = ~50 seconds on testnet)
sleep 60

# Finalize reputation
FINAL_TX=$(curl -s -X POST http://localhost:8080/finalize-reputation \
  -H "Content-Type: application/json" \
  -d "{
    \"reputation_cell\": \"...\",
    \"worker_lock_args\": \"0x...\"
  }" | jq -r '.tx_hash')

echo "Finalized reputation with tx: $FINAL_TX"

# Verify reputation cell updated
curl -s http://localhost:8080/get-reputation?lock_args=0x... | jq '.reputation'
```

**Expected results:**
- Reputation cell created with proposal
- Dispute window enforced (transaction fails if submitted before timeout)
- Reputation finalized after window closes
- completed/abandoned count incremented correctly

### 5. Signing Backend Equivalence Test

Verify that LocalSigner and SuperiseSigner produce identical signatures.

```bash
# 1. Generate a test transaction
TEST_TX='{
  "inputs": [...],
  "outputs": [...],
  "witnesses": [...],
  "version": "0"
}'

# 2. Sign with LocalSigner (port 8080)
LOCAL_SIG=$(curl -s -X POST http://localhost:8080/sign-tx \
  -H "Content-Type: application/json" \
  -d "$TEST_TX" | jq -r '.signature')

# 3. Sign with SupeRISE (port 8090 after switching)
SUPERISE_SIG=$(curl -s -X POST http://localhost:8090/sign-tx \
  -H "Content-Type: application/json" \
  -d "$TEST_TX" | jq -r '.signature')

# 4. Compare signatures
if [ "$LOCAL_SIG" = "$SUPERISE_SIG" ]; then
  echo "✓ Signatures match (same tx_hash and witness count)"
else
  echo "✗ Signatures differ - check witness construction"
fi
```

**Expected results:**
- Both signers produce identical 65-byte signatures for the same transaction
- Signing message computation identical
- Witness encoding identical

### Important Note: Dispute Testing

**Not covered in v1 tests:**
- Automated dispute submission (not implemented)
- Dispute resolution arbitration (not implemented)
- Result verification beyond off-chain hash comparison (not implemented)
- Slashing conditions (not implemented)

**What IS tested:**
- Result hash storage in settlement
- Reputation dispute window enforcement (prevents early finalization)
- Off-chain result verification (poster manually checks hash match)

See "Known Limitations" in README.md for details.

---

## Test Results Summary

After completing all tests, verify:

- ✓ Unit tests: 58 tests pass, zero warnings
- ✓ LocalSigner mode: all endpoints functional
- ✓ SuperiseSigner mode: all endpoints functional
- ✓ Marketplace flow: 4 transactions confirm correctly
- ✓ Capability proof: attestation correctly signed and encoded
- ✓ Reputation flow: dispute window enforced, finalization works
- ✓ Signing equivalence: both backends produce identical signatures
- ✓ lock_args consistency: derived correctly in both modes

## Troubleshooting

| Issue | Diagnosis | Solution |
|-------|-----------|----------|
| "Connection refused" on /balance | nerve-core not running | Start `cargo run -p nerve-core` in terminal 1 |
| "Invalid lock_args" error | Signer not initialized | Check SIGNING_BACKEND env var and private key format |
| "SupeRISE timeout" | MCP endpoint unreachable | Verify SupeRISE running and SUPERISE_URL correct |
| Signature mismatch | Different witness construction | Check placeholder_witness() is being called with same inputs |
| Tx broadcast fails | Testnet RPC issue | Verify CKB_RPC_URL is reachable and returns blocks |
| Reputation finalization fails | Dispute window not elapsed | Wait full window (default 10 blocks ≈ 50 seconds) |

## Continuous Integration

All unit tests run automatically on:
- Commits to main branch
- Pull requests
- Docker build pipeline

Command to run locally:
```bash
cd packages/core
cargo test --all-features
```

---

**Test Plan Version:** 1.0
**Last Updated:** March 2026
**Coverage:** LocalSigner, SuperiseSigner, marketplace flows, capability proofs, reputation updates
