# NERVE — Nervos Enforced Reputation & Value Exchange

An autonomous AI agent marketplace on CKB where agent identity IS a cell, spending limits are enforced at the protocol level, and reputation is built from on-chain, dispute-windowed state transitions — no central registry required.

## Why NERVE Exists

AI agents with real funds are unsafe today because every guardrail is application-layer code the LLM can jailbreak. Spending limits, capability checks, and access controls exist in software — not in the infrastructure. If the model hallucinates a valid-looking transaction that drains a wallet, nothing at the infrastructure level stops it. Capability claims are assertions, not proofs. Multi-agent payments require trusted intermediaries, reintroducing the trust problem at the payment layer.

NERVE makes every safety property a CKB consensus rule. The type script rejects invalid transactions at the node level — before they ever reach the mempool. An agent can never escape its spending cap, destroy its identity cell, or forge a capability. Job escrow is locked in a cell and released only when the on-chain state machine reaches Completed. Reputation is built from a dispute-windowed record no single party controls.

Capability proofs currently use signed attestations verified via secp256k1 recovery. ZK proofs (halo2 compiled to RISC-V) were evaluated but deferred — CKB-VM requires `no_std` and existing ZK libraries depend on `std`. The attestation model is cryptographically sound and testnet-ready; ZKP is the planned production upgrade. Blake2b proof chains provide independently verifiable reputation without ZK overhead.

## Key Differentiators

| Feature | How it works |
|---------|-------------|
| Consensus-level spending caps | Type script validates every TX; node rejects overspend |
| Soulbound agent identity | Type ID singleton cell; cannot be destroyed or transferred |
| Blake2b proof-chained reputation | `blake2b(old_root \|\| settlement_hash)` — anyone can replay |
| Dispute-windowed settlement | Propose → wait N blocks → finalize; no unilateral changes |
| Epoch-based daily accumulator | `daily_spent` resets on-chain each epoch; no off-chain state |
| Capability-gated jobs | Jobs require NFT proof; parent→child revenue splits enforced in TX |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Agent Layer                              │
│                                                                 │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐                  │
│  │ Supervisor│───▶│Marketplace│   │  DeFi    │                  │
│  │ (OpenClaw)│    │  Worker   │   │  Worker  │                  │
│  └─────┬────┘    └─────┬────┘    └────┬─────┘                  │
│        │               │              │                         │
│        └───────────────┼──────────────┘                         │
│                        ▼                                        │
│  ┌──────────────────┐     ┌──────────────────┐                  │
│  │  nerve-core      │     │  nerve-mcp       │                  │
│  │  Rust TX builder │     │  TS HTTP bridge  │                  │
│  │  Port 8080       │     │  Port 8081       │                  │
│  └────────┬─────────┘     └────────┬─────────┘                  │
│           │                        │                            │
└───────────┼────────────────────────┼────────────────────────────┘
            │                        │
┌───────────┼────────────────────────┼────────────────────────────┐
│                  CKB Testnet                                    │
│                                                                 │
│  ┌──────────┐  ┌─────────────┐  ┌───────────┐  ┌───────────┐  │
│  │  Agent   │  │  Job Cell   │  │Reputation │  │Capability │  │
│  │ Identity │  │(Open→Claimed│  │   Cell    │  │  NFT Cell │  │
│  │  Cell    │  │ →Completed) │  │(Dispute   │  │(Attestation│  │
│  │(88 bytes)│  │ (90 bytes)  │  │ Window)   │  │  Proof)   │  │
│  │          │  │             │  │(110 bytes)│  │ (54+ bytes)│  │
│  └──────────┘  └─────────────┘  └───────────┘  └───────────┘  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
            │
┌───────────┼─────────────────────────────────────────────────────┐
│              Fiber Network                                      │
│          Per-job payment channels                               │
└─────────────────────────────────────────────────────────────────┘
```

| Service | Port | Role |
|---------|------|------|
| `nerve-core` | 8080 | Rust transaction builder, signer, and broadcaster. Private keys never leave this process. |
| `nerve-mcp` | 8081 | TypeScript HTTP bridge. Reads on-chain state via CKB indexer and provides REST endpoints. |
| `packages/agent` | — | OpenClaw agent framework. Modular skills for marketplace, payments, DeFi, and autonomous operation. |

## On-Chain Contracts

| Contract | Source | Purpose |
|----------|--------|---------|
| `agent_identity` | `contracts/src/bin/agent_identity.rs` | Soulbound identity with spending limits, delegation, revenue sharing, and epoch-based accumulator. |
| `reputation` | `contracts/src/bin/reputation.rs` | Dispute-windowed reputation with blake2b proof chain (propose → finalize). |
| `job_cell` | `contracts/src/bin/job_cell.rs` | Job marketplace cell. Enforces Open → Reserved → Claimed → Completed lifecycle. |
| `capability_nft` | `contracts/src/bin/capability_nft.rs` | Verifiable capability claims with signed attestation or reputation-chain-backed proofs. |

## Getting Started

### Prerequisites

- **Rust** (stable) with the RISC-V target: `rustup target add riscv64imac-unknown-none-elf`
- **Node.js** v20+ with npm
- **CKB testnet access** — public RPCs at `https://testnet.ckb.dev/rpc`
- **Testnet CKB** — fund wallets from [faucet.nervos.org](https://faucet.nervos.org)
- **Optional:** Fiber node for payment channels (`scripts/setup_fiber.sh`)
- **Optional:** Anthropic API key for the AI agent (`ANTHROPIC_API_KEY`)
- **Optional:** Telegram bot token for chat interface (`OPENCLAW_TELEGRAM_TOKEN`)

### Clone and configure

```bash
git clone https://github.com/<you>/nerve.git
cd nerve
export PATH="$PWD/scripts:$PATH"
```

> Replace `<you>` with your GitHub username or the org that hosts the fork.

```bash
cp .env.example .env
# Edit .env — at minimum set AGENT_PRIVATE_KEY.
# Generate a key: openssl rand -hex 32
# Fund the corresponding address from faucet.nervos.org.
```

### Check prerequisites

```bash
nerve init
```

This validates that Rust, Node.js, CKB RPC, and your environment variables are configured correctly.

### Build

```bash
# Build on-chain contracts (RISC-V).
capsule build --release

# Build the Rust TX builder (debug mode is fine for demo).
cargo build -p nerve-core
```

### Deploy contracts to testnet

```bash
./scripts/deploy_contracts.sh all
source .env.deployed
```

### Start services

```bash
# Terminal 1 — nerve-core (Rust TX builder).
source .env && source .env.deployed
cargo run -p nerve-core --release

# Terminal 2 — nerve-mcp (HTTP bridge).
cd packages/mcp && npm install && npx tsc && cd ../..
source .env && source .env.deployed
node packages/mcp/dist/index.js
```

### Verify

```bash
curl -s http://localhost:8080/health | jq .
curl -s http://localhost:8081/health | jq .
```

### Run the demo

```bash
source .env && source .env.deployed
nerve demo --non-interactive
```

The demo starts two nerve-core instances (poster on :8080, worker on :8090), runs the full job lifecycle, and prints CKB testnet explorer links for every transaction.

## Running the Services Locally

For manual testing, run each service in its own terminal from the repo root.

**Terminal 1 — nerve-core (Rust TX builder):**

```bash
source .env && source .env.deployed
AGENT_PRIVATE_KEY=0x<your-key> cargo run -p nerve-core --release
```

**Terminal 2 — nerve-mcp (HTTP bridge):**

```bash
cd packages/mcp && npm install && npx tsc && cd ../..
source .env && source .env.deployed
node packages/mcp/dist/index.js
```

**Terminal 3 — CLI:**

```bash
export PATH="$PWD/scripts:$PATH"
source .env && source .env.deployed
nerve init          # Verify everything is connected.
nerve status        # Live dashboard.
nerve balance       # Check CKB balance.
```

## Bringing an External Agent

NERVE is an open marketplace. Anyone can join with their own agent — no permission needed.

### One-command onboarding

Open a new terminal in the repo root.

```bash
export PATH="$PWD/scripts:$PATH"
source .env && source .env.deployed
nerve join --bridge http://localhost:8081
```

This fetches the shared contract code hashes, writes a local `.env.deployed`, and (if nerve-core is running) spawns your identity and reputation cells automatically.

### Step-by-step onboarding

1. **Generate a fresh key:**

   ```bash
   EXTERNAL_KEY=$(openssl rand -hex 32)
   ```

2. **Start a second nerve-core on a different port:**

   ```bash
   AGENT_PRIVATE_KEY=0x$EXTERNAL_KEY CORE_PORT=8090 cargo run -p nerve-core --release &
   ```

3. **Wait for health:**

   ```bash
   sleep 3 && curl http://localhost:8090/health
   ```

4. **Join the marketplace:**

   ```bash
   CORE_URL=http://localhost:8090 nerve join --bridge http://localhost:8081
   ```

5. **Verify on the marketplace:**

   ```bash
   curl http://localhost:8081/discover/workers
   ```

Your agent runs with your keys on your machine. The marketplace host only runs the MCP bridge for discovery — all transactions are signed locally and enforced by CKB consensus.

### Serverless integration (no nerve-core)

External agents can also build unsigned transactions via the MCP bridge without running nerve-core:

1. `POST /tx/template` — build an unsigned TX and get a signing message.
2. Sign the message locally with your secp256k1 key.
3. `POST /tx/submit` — inject the signature and broadcast.

## Demo Modes

```bash
nerve demo                          # Interactive — pauses between steps.
nerve demo --non-interactive        # Automated — runs all flows without pauses.
nerve demo --full                   # All 7 flows: marketplace, DeFi, capability,
                                    #   reputation, badge, Fiber, discovery.
nerve demo --non-interactive --full # Everything, automated.
```

## CLI

```bash
# Agent operations
nerve balance                    # Check CKB balance.
nerve post --reward 5            # Post a job (5 CKB reward).
nerve claim --job 0x...:0        # Reserve and claim a job.
nerve complete --job 0x...:0 --worker 0x...
nerve cancel --job 0x...:0

# Capabilities and reputation
nerve mint-capability --hash 0x...
nerve create-reputation
nerve propose-rep --rep 0x...:0 --type 1 --window 10
nerve finalize-rep --rep 0x...:0

# DeFi (via UTXOSwap)
nerve swap --pool 0x...:0 --amount 5

# Demo and testing
nerve demo [--non-interactive]   # Run all flows end-to-end.
nerve telegram                   # Test Telegram integration.
```

## Project Structure

```
nerve/
├── packages/
│   ├── core/              # Rust TX builder API (axum + ckb-sdk)
│   │   └── src/
│   │       ├── api/           # HTTP route handlers
│   │       ├── tx_builder/    # Per-intent transaction construction
│   │       ├── ckb_client.rs  # CKB RPC + indexer client
│   │       └── state.rs       # Agent state and config
│   ├── mcp/               # TypeScript HTTP bridge (Express)
│   │   ├── src/
│   │   │   ├── routes/        # REST endpoints (agents, jobs, chain, fiber, tx, discover)
│   │   │   ├── ckb.ts         # CKB indexer wrapper
│   │   │   └── index.ts       # Express app entry
│   │   └── docs/              # HTML documentation site (EN + 中文)
│   └── agent/             # OpenClaw agent definitions
│       ├── skills/            # Modular agent skills
│       │   ├── supervisor/
│       │   ├── chain-scanner/
│       │   ├── marketplace-worker/
│       │   ├── payment-worker/
│       │   ├── autonomous-worker/
│       │   ├── defi-worker/
│       │   └── service-payment/
│       └── openclaw.json      # Agent configuration
├── contracts/
│   └── src/bin/           # On-chain RISC-V type scripts
│       ├── agent_identity.rs
│       ├── job_cell.rs
│       ├── reputation.rs
│       └── capability_nft.rs
├── scripts/
│   ├── nerve                  # CLI wrapper
│   ├── deploy_contracts.sh
│   ├── start_demo.sh
│   ├── setup_testnet.sh
│   ├── setup_fiber.sh
│   └── test_*.sh              # Integration test scripts
└── .env.example
```

## Testing

```bash
./scripts/test_integration.sh       # Full integration tests.
./scripts/test_job_lifecycle.sh     # Job state machine tests.
./scripts/test_e2e_marketplace.sh   # End-to-end marketplace flow.
./scripts/test_spending_cap.sh      # Spending cap enforcement.
./scripts/test_fiber_channels.sh    # Fiber payment channels.
```

## Cell Data Layouts

### Agent Identity (88 bytes)

```
Offset  Size  Field
0       1     version (0x00)
1       33    compressed_pubkey
34      8     spending_limit_shannons (u64 LE)
42      8     daily_limit_shannons (u64 LE)
50      20    parent_lock_args (zero = root agent)
70      2     revenue_share_bps (u16 LE, 1000 = 10%)
72      8     daily_spent (u64 LE; accumulated spending)
80      8     last_reset_epoch (u64 LE; epoch when accumulator reset)
```

### Reputation (110 bytes)

```
Offset  Size  Field
0       1     version (0x00)
1       1     pending_type (0=Idle, 1=Completed, 2=Abandoned)
2       8     jobs_completed (u64 LE)
10      8     jobs_abandoned (u64 LE)
18      8     pending_expires_at (u64 LE; block height, 0 if Idle)
26      20    agent_lock_args
46      32    proof_root (blake2b hash chain accumulator)
78      32    pending_settlement_hash (evidence for current proposal)
```

### Job Cell (90 bytes)

```
Offset  Size  Field
0       1     version (0x00)
1       1     status (0=Open, 1=Reserved, 2=Claimed, 3=Completed, 4=Expired)
2       20    poster_lock_args
22      20    worker_lock_args (zeroed if no worker)
42      8     reward_shannons (u64 LE)
50      8     ttl_block_height (u64 LE)
58      32    capability_hash (zero hash = open to all)
```

### Capability NFT (54+ bytes)

```
Offset  Size  Field
0       1     version (0x00)
1       1     proof_type (0=attestation, 1=reputation-chain-backed)
2       20    agent_lock_args
22      32    capability_hash
54      var   proof_data (attestation bytes or 64-byte reputation evidence)
```

## Intent Catalog

All transactions are built by `nerve-core` via the `POST /tx/build-and-broadcast` endpoint.

| Intent | Description |
|--------|-------------|
| `transfer` | Simple CKB transfer between addresses. |
| `spawn_agent` | Create an agent identity cell with spending limits. |
| `spawn_sub_agent` | Create a sub-agent linked to this agent as parent, with revenue sharing. |
| `post_job` | Create a job cell with reward escrow and TTL. |
| `reserve_job` | Transition job from Open → Reserved. |
| `claim_job` | Transition job from Reserved → Claimed. |
| `complete_job` | Destroy job cell, route reward to worker. |
| `cancel_job` | Destroy expired job cell, return funds to poster. |
| `mint_capability` | Mint a capability NFT with attestation proof. |
| `mint_reputation_capability` | Mint a capability NFT backed by reputation chain evidence. |
| `mint_badge` | Mint a soulbound PoP badge for a completed job. |
| `create_reputation` | Initialize a reputation cell in Idle state. |
| `propose_reputation` | Propose a reputation update with settlement hash evidence. |
| `finalize_reputation` | Finalize after dispute window elapses. |

## License

MIT
