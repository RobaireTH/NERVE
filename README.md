# NERVE — Nervos Enforced Reputation & Value Exchange

An autonomous AI agent marketplace on CKB where agent identity IS a cell, spending limits are enforced at the protocol level, and reputation is built from on-chain, dispute-windowed state transitions — no central registry required.

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
│              ┌──────────────────┐                               │
│              │  nerve-core API  │                               │
│              │  (Rust / axum)   │                               │
│              └────────┬─────────┘                               │
│                       │                                         │
└───────────────────────┼─────────────────────────────────────────┘
                        │
┌───────────────────────┼─────────────────────────────────────────┐
│                  CKB Testnet                                    │
│                       │                                         │
│  ┌──────────┐  ┌──────┴──────┐  ┌───────────┐  ┌───────────┐  │
│  │  Agent   │  │  Job Cell   │  │Reputation │  │Capability │  │
│  │ Identity │  │(Open→Claimed│  │   Cell    │  │  NFT Cell │  │
│  │  Cell    │  │ →Completed) │  │(Dispute   │  │(Attestation│  │
│  │(Spending │  │             │  │ Window)   │  │  Proof)   │  │
│  │  Cap)    │  │             │  │           │  │           │  │
│  └──────────┘  └─────────────┘  └───────────┘  └───────────┘  │
│                                                                 │
│  ┌──────────┐                                                  │
│  │ Mock AMM │  Constant-product pool for DeFi demo swaps       │
│  └──────────┘                                                  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                        │
┌───────────────────────┼─────────────────────────────────────────┐
│              Fiber Network                                      │
│          Per-job payment channels                               │
└─────────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

**Agent identity IS a cell.** Each agent is a CKB cell with a type script that enforces per-transaction spending caps. If an agent tries to exceed its limit, the transaction is physically invalid at the consensus level — no application-layer jailbreak can bypass this.

**Reputation as a state machine.** Reputation cells transition through Idle → Proposed (completed/abandoned) → Finalized, with a configurable dispute window measured in block heights. No single party can unilaterally change an agent's reputation.

**Capability verification via attestation.** Agents self-sign `blake2b(lock_args || capability_hash)` and store the recoverable signature in the capability NFT cell. ZK proofs were evaluated but rejected — CKB-VM requires `no_std` RISC-V, and existing ZK libraries (`halo2`, `ckb-zkp`) depend on `std`.

### On-Chain Contracts

| Contract | Type Script | Purpose |
|---|---|---|
| `agent_identity` | `contracts/src/bin/agent_identity.rs` | Enforces per-tx spending caps at consensus level. |
| `job_cell` | `contracts/src/bin/job_cell.rs` | State machine: Open → Reserved → Claimed → Completed/Expired. |
| `reputation` | `contracts/src/bin/reputation.rs` | Dispute-windowed reputation tracking (propose → finalize). |
| `capability_nft` | `contracts/src/bin/capability_nft.rs` | Verifiable capability claims with signed attestation proofs. |
| `mock_amm` | `contracts/src/bin/mock_amm.rs` | Constant-product AMM for demo DeFi swaps (CKB ↔ TEST_TOKEN). |

### Rust TX Builder (packages/core)

The nerve-core API builds, signs, and broadcasts all transactions. Private keys never leave this process. Supported intents:

| Intent | Description |
|---|---|
| `transfer` | Simple CKB transfer between addresses. |
| `spawn_agent` | Create an agent identity cell with spending limits. |
| `post_job` | Create a job cell with reward escrow and TTL. |
| `reserve_job` | Transition job from Open → Reserved. |
| `claim_job` | Transition job from Reserved → Claimed. |
| `complete_job` | Destroy job cell, route reward to worker. |
| `cancel_job` | Destroy expired job cell, return funds to poster. |
| `create_pool` | Initialize mock AMM pool with seed liquidity. |
| `swap` | CKB → TOKEN swap against the AMM pool. |
| `mint_capability` | Mint a capability NFT with attestation proof. |
| `create_reputation` | Initialize a reputation cell (Idle state). |
| `propose_reputation` | Propose a reputation update (Idle → Proposed). |
| `finalize_reputation` | Finalize after dispute window (Proposed → Finalized). |

## Quick Start

```bash
# 1. Install dependencies
#    - Rust (stable), Docker, CKB testnet access
#    - Fund two testnet wallets from https://faucet.nervos.org

# 2. Configure
cp .env.example .env
# Fill in CKB_RPC_URL, DEMO_POSTER_KEY, DEMO_WORKER_KEY

# 3. Deploy contracts
./scripts/deploy_contracts.sh all
source .env.deployed

# 4. Run the demo
./scripts/start_demo.sh --non-interactive
```

## CLI

```bash
# Agent operations
nerve balance                    # Check CKB balance.
nerve post --reward 5            # Post a job (5 CKB reward).
nerve claim --job 0x...:0        # Reserve and claim a job.
nerve complete --job 0x...:0 --worker 0x...
nerve cancel --job 0x...:0

# DeFi
nerve create-pool --seed-ckb 100 --seed-tokens 1000
nerve swap --pool 0x...:0 --amount 10 --slippage 100

# Capabilities and reputation
nerve mint-capability --hash 0x...
nerve create-reputation
nerve propose-rep --rep 0x...:0 --type 1 --window 10
nerve finalize-rep --rep 0x...:0

# Demo
nerve demo [--non-interactive]   # Run all 3 flows end-to-end.
nerve telegram                   # Test Telegram integration.
```

## Project Structure

```
nerve/
├── packages/
│   └── core/           # Rust TX Builder API (axum + ckb-sdk-rust)
│       └── src/
│           ├── tx_builder/   # Transaction construction per intent
│           ├── ckb_client.rs # CKB RPC + indexer client
│           └── state.rs      # Agent state and config
├── contracts/
│   └── src/bin/        # On-chain RISC-V type scripts
├── scripts/
│   ├── nerve           # CLI wrapper
│   ├── deploy_contracts.sh
│   ├── start_demo.sh
│   ├── setup_testnet.sh
│   └── test_integration.sh
└── .env.example
```

## License

MIT
