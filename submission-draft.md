# NERVE — Submission Draft

> Nervos Enforced Reputation & Value Exchange
>
> An autonomous AI agent marketplace on CKB where agent identity IS a cell, spending limits are enforced at the protocol level, and reputation is built from on-chain, dispute-windowed state transitions.

---

## 1. Executive Summary

NERVE is a protocol-level agent marketplace on CKB. Three core safety properties are enforced as consensus rules, not application code:

1. **Spending caps are protocol-enforced.** A type script validates every transaction; if an agent exceeds its per-transaction limit, the node rejects it. No jailbreak can override consensus.

2. **Agent identity is a cell.** An agent's identity, capabilities, reputation, and spending rules are all encoded in a single CKB cell. Transfer the cell = transfer the agent. All state is on-chain and immutable by decree of the lock script.

3. **Reputation is dispute-windowed and chain-linkable.** Job completions are recorded on-chain via a blake2b proof chain (propose → wait N blocks → finalize). No single party can unilaterally change reputation; challenges require on-chain proof.

The marketplace operates end-to-end without a central registry. Agents discover each other, post jobs, stream payments over Fiber Network, and prove capabilities via signed attestations (v1) or ZK proofs (v2).

**Key technical contribution:** On-chain state machine for job lifecycle (Open → Reserved → Claimed → Completed) with UTXO atomicity preventing double-claims. Spending limits enforced at consensus level, making it commercially viable to deploy LLM agents with real funds.

---

## 2. Why This Problem Matters

Current AI agent systems have three critical structural problems:

### 2.1 Guardrails are Application-Layer

Spending limits, capability checks, and access controls are code that can be jailbroken or confused by an LLM. If Claude hallucinates a valid-looking transaction, nothing at the infrastructure level stops it from draining a wallet. The guardrail enforcement is a policy, not a physical law.

### 2.2 Agent Identity is Off-Chain and Mutable

There is no trustless way to verify what an agent can do, what its track record is, or who controls it. Capability claims are assertions made by code, not proofs anchored on-chain. Reputation systems are databases operated by platforms that can be deleted or rewritten.

### 2.3 Multi-Agent Payments Require Trusted Intermediaries

When agents hire other agents, they need escrow, settlement, and reputation verification — which reintroduces the trust problem at the payment layer. The hired agent must trust the hiring agent (or a central platform) to route funds correctly.

### 2.4 Why CKB Specifically Solves This

- **Cell model / UTXO:** Agent identity IS a cell. Ownership is native and transferable. No contract registry to shut down, no admin key to revoke.
- **RISC-V VM:** Arbitrary computation on-chain without precompile constraints (unlike EVM). Future ZK proofs run natively.
- **Fiber Network:** Micropayment channels enable per-action billing at scale.
- **Blake2b proof chains:** Verifiable reputation updates without ZK overhead (current), with ZK for v2.

---

## 3. Architecture & System Design

### 3.1 End-to-End Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Layer                               │
│  CLI (bash) / Telegram / Slack / Discord (via OpenClaw)         │
└────────────────────┬────────────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────────────┐
│              Agent Runtime (OpenClaw)                            │
│  Supervisor Skill (Claude Opus)                                  │
│    ├─ Marketplace Worker (job discovery, claim, settle)          │
│    ├─ DeFi Worker (UTXOSwap integration)                        │
│    ├─ Payment Worker (Fiber channels)                           │
│    └─ Chain Scanner (autonomous heartbeat every 10 min)         │
│                                                                  │
│  State: OpenClaw Memory (Markdown checkpoints + plan state)     │
└──────────┬───────────────────────┬──────────────────────────────┘
           │ HTTP REST             │ HTTP REST
┌──────────▼──────────┐  ┌────────▼────────────────┐
│  nerve-core         │  │  nerve-mcp              │
│  (Rust / axum)      │  │  (TypeScript / Express) │
│                     │  │                         │
│  • LocalSigner or   │  │  • CKB RPC / Indexer   │
│    SuperiseSigner   │  │  • Fiber JSON-RPC      │
│  • TX construction  │  │  • Cell scanning        │
│  • secp256k1 sign   │  │  • Event indexing       │
│  Port: 8080         │  │  Port: 8081             │
└──────────┬──────────┘  └────────┬────────────────┘
           │                      │
└──────────┼──────────────────────┼──────────────────┐
           │                      │                  │
        ┌──▼──────────────────────▼───────────────┐  │
        │                                         │  │
        │      CKB Testnet                        │  │
        │                                         │  │
        │  Agent Identity (type script enforces  │  │
        │  spending caps, burn protection,       │  │
        │  Type ID uniqueness)                   │  │
        │                                         │  │
        │  Job Cells (FSM: Open → Reserved →    │  │
        │  Claimed → Completed, UTXO atomicity) │  │
        │                                         │  │
        │  Reputation Cells (dispute-windowed,   │  │
        │  blake2b proof chain)                  │  │
        │                                         │  │
        │  Capability NFTs (signed attestation   │  │
        │  or ZK proofs, immutable once issued)  │  │
        │                                         │  │
        └─────────────────────────────────────────┘  │
                                                     │
        ┌────────────────────────────────────────┐  │
        │  Fiber Network                         │  │
        │  Per-job payment channels              │  │
        │  P2P gossip (future: job discovery)    │  │
        └────────────────────────────────────────┘  │
                                                     │
        ┌────────────────────────────────────────┐  │
        │  UTXOSwap (DeFi demo)                  │  │
        │  Token swaps + RGB++ integration       │  │
        └────────────────────────────────────────┘  │
```

### 3.2 Data Structures

#### Agent Identity Cell

```
cell {
  capacity: <locked CKB>
  lock: secp256k1-blake2b (20-byte pubkey hash)
  type: agent_identity (enforces spending caps + Type ID)
  data: v1 (72 bytes)
    version: u8
    pubkey: [u8; 33]
    spending_limit_per_tx: u64
    daily_limit: u64
    epoch_started: u64
    daily_spent: u64
    parent_lock_args: Option<[u8; 20]>
    revenue_share_bps: u16
}
```

**Type script enforces:**
- Spending cap per transaction (burning to non-self outputs)
- Burn protection (identity cell must reappear in outputs)
- Type ID singleton (one identity cell per agent)
- Daily accumulator reset on epoch boundary

#### Job Cell

```
cell {
  capacity: reward_ckb + 122 (minimum)
  lock: time-locked posterior lock (TTL-based)
  type: job_cell (enforces state FSM)
  data: ~122 bytes
    job_id: [u8; 32]
    status: u8 (Open=0, Reserved=1, Claimed=2, Completed=3)
    poster_lock_args: [u8; 20]
    worker_lock_args: Option<[u8; 20]> (set on claim)
    reward_ckb: u64
    ttl_block: u64
    created_block: u64
    capability_required_hash: [u8; 32]
}
```

**Type script enforces:**
- Adjacent state transitions (Open → Reserved → Claimed → Completed)
- Poster and reward immutable after creation
- UTXO atomicity: only one tx can consume this cell (first-write-wins)

#### Reputation Cell

```
cell {
  capacity: 110 (minimum)
  lock: secp256k1-blake2b + dispute timelock
  type: reputation (dispute-windowed updates)
  data: ~110 bytes
    agent_lock_args: [u8; 20]
    completed_count: u64
    abandoned_count: u64
    score: u64 (composite)
    pending_hash: Option<[u8; 32]>
    pending_block: u64
    proof_root: [u8; 32] (blake2b chain)
}
```

**Type script enforces:**
- Dispute window before finalization (N blocks configurable)
- Timestamp freshness (pending must be older than N blocks to finalize)
- Proof chain: `blake2b(old_root || new_update_hash)`

#### Capability NFT Cell

```
cell {
  capacity: 61 (minimum)
  lock: agent's lock script
  type: capability_nft (immutable once issued)
  data: ~54 bytes
    agent_lock_args: [u8; 20]
    capability_hash: [u8; 32]
    attestation_sig: [u8; 65] (recoverable secp256k1)
    attestation_msg: blake2b(lock_args || capability_hash)
}
```

**Type script enforces:**
- Immutable agent_lock_args and capability_hash
- Cell cannot be destroyed
- Verification: recover pubkey from sig, confirm matches agent identity

### 3.3 Key Flows

#### Agent Identity Spawning

1. User calls `nerve init` with private key
2. TX Builder derives lock_args from pubkey hash
3. Constructs identity cell with Type ID singleton guarantee
4. Broadcasts to testnet
5. Identity cell now governs spending rules at consensus level

#### Job Marketplace (Open → Reserved → Claimed → Completed)

1. **Poster posts job:**
   - TX: create job cell, capacity = reward + minimum, state = Open
   - On-chain: reward escrowed in cell capacity

2. **Worker reserves job:**
   - TX: consume job cell, produce new job cell with state = Reserved, worker_lock_args = None (soft-lock in on-chain registry)
   - Off-chain registry grants exclusive window to prevent wasted fees

3. **Worker claims job:**
   - TX: consume job cell, produce new job cell with state = Claimed, worker_lock_args = worker's lock
   - UTXO atomicity: if two workers submit simultaneously, only one succeeds (consensus-level guarantee)

4. **Poster settles job:**
   - TX: consume job cell, destroy it, output to worker address with reward
   - Concurrent: write reputation update cell (Open → Proposed)

5. **Reputation finalization:**
   - TX: consume reputation cell after dispute window closes, produce new reputation cell with Finalized count

#### Capability Proof Issuance

1. Agent generates capability_hash = blake2b(description)
2. Creates attestation signature = sign(lock_args || capability_hash)
3. TX: create capability NFT cell with signature embedded
4. On-chain: anyone can verify: recover(sig, msg) = agent's pubkey

#### DeFi Execution (UTXOSwap Integration)

1. Agent requests swap via Telegram ("swap 50 CKB for USDT")
2. Supervisor routes to DeFi Worker
3. DeFi Worker:
   - Queries UTXOSwap for pools and rates
   - Constructs swap TX via Rust TX Builder
   - TX applies to agent's identity cell with spending cap check
4. TX broadcasts → confirmed on testnet
5. Agent receives USDT, optionally streams to peer via Fiber

---

## 4. Recent Development: SupeRISE Signing Backend Integration

### 4.1 Problem Solved

NERVE originally held agent private keys directly in the `AGENT_PRIVATE_KEY` environment variable, loaded into the Rust core at startup. This works for testing but creates security concerns for production:

- Private key in plaintext env var (even if file permissions restrict access)
- No hardware wallet support
- Difficult to rotate keys or manage multiple agents
- No encryption at rest

### 4.2 Solution: Pluggable Signing Backends

Added a trait-based abstraction (`Signer`) with two implementations:

**LocalSigner (default, existing behavior):**
- Wraps secp256k1 signing
- Holds private key in memory
- Fast (in-process)
- Suitable for development and testing

**SuperiseSigner (new, optional):**
- Delegates to SupeRISE wallet service (separate process)
- Private key never leaves SupeRISE (AES-256-GCM encrypted at rest)
- Supports hardware wallets via SupeRISE
- ~50ms RPC latency per signature

### 4.3 Implementation Details

**New file:** `packages/core/src/signer.rs` (362 lines)

```rust
#[async_trait]
pub trait Signer: Send + Sync {
    /// Sign a CKB transaction. Returns 65-byte recoverable ECDSA signature.
    async fn sign(&self, tx_hash: &str, witness_count: usize)
        -> Result<[u8; 65], TxBuildError>;

    /// Sign with a custom first witness (for input_type data).
    async fn sign_with_witness(&self, tx_hash: &str, first_witness: &[u8], witness_count: usize)
        -> Result<[u8; 65], TxBuildError>;

    /// Sign an attestation message (for capability NFT proofs).
    async fn attest(&self, message: &[u8; 32])
        -> Result<Vec<u8>, TxBuildError>;

    /// Return the compressed public key.
    async fn pubkey(&self) -> Result<[u8; 33], TxBuildError>;

    /// Return the lock_args for this signer.
    fn lock_args(&self) -> &str;
}
```

**LocalSigner:**
- Wraps existing `sign_tx()` and `sign_tx_with_witness()` logic
- Holds `private_key: Vec<u8>` and `lock_args: String`
- Derives lock_args from private key at initialization

**SuperiseSigner:**
- HTTP client calls SupeRISE's MCP endpoint
- On init: calls `nervos.address` to get address, derives lock_args via bech32 decoding
- `sign()`: computes signing message locally, calls `nervos.sign_message` with message hash
- Preserves `compute_signing_message()` logic (shared between both signers)

### 4.4 Backend Selection

Environment variable `SIGNING_BACKEND` (default: `local`):

```bash
# Mode 1: Local (default, existing behavior)
SIGNING_BACKEND=local
AGENT_PRIVATE_KEY=0x<32-byte-hex>

# Mode 2: SupeRISE (new, optional)
SIGNING_BACKEND=superise
SUPERISE_URL=http://127.0.0.1:18799/mcp
# AGENT_PRIVATE_KEY not required
```

### 4.5 Signing Flow (Both Modes)

1. **TX Builder constructs unsigned tx**
   - Inputs, outputs, witnesses (with 65-byte placeholder in lock field)

2. **Compute signing message**
   - `blake2b(tx_hash || len(witness[0]) || witness[0] || ...)`
   - Identical for both signers (defined in `compute_signing_message()`)

3. **Sign via Signer trait**
   - LocalSigner: secp256k1 sign in-process
   - SuperiseSigner: HTTP RPC call to SupeRISE
   - Both return: 65-byte recoverable signature

4. **Inject signature**
   - Replace placeholder [0u8; 65] at offset [20..85] in witness
   - Broadcast to testnet

### 4.6 Code Changes Summary

| File | Change |
|------|--------|
| `packages/core/src/signer.rs` | NEW: Signer trait + LocalSigner + SuperiseSigner (362 lines) |
| `packages/core/src/state.rs` | Replace `private_key: Vec<u8>` with `signer: Arc<dyn Signer>`, async from_env() |
| `packages/core/src/main.rs` | Add `mod signer`, make main async |
| `packages/core/src/tx_builder/*.rs` | All builders: `sign_tx(...)` → `state.signer.sign(...).await?` |
| `packages/core/Cargo.toml` | Add `async-trait` dependency |
| `.env.example` | Document SIGNING_BACKEND and SUPERISE_URL |

### 4.7 Testing & Verification

**Unit tests:** All 58 existing tests pass without modification
- Signing tests verify placeholder witness construction
- Recovery ID validation (0 or 1)
- Signature injection into witness
- Blake2b proof chain

**Integration test plan:** See `TEST.md` for comprehensive testing guide

**Backward compatibility:** LocalSigner is default; no breaking changes to API or on-chain transaction format

---

## 5. Current Functionality & Demo Flows

### 5.1 Three End-to-End Demo Flows

All flows are fully functional and testable against CKB testnet.

#### Flow 1: Agent Marketplace

```bash
nerve demo --flow marketplace
```

- Poster posts job: "Summarize this document, reward 5 CKB, TTL 100 blocks"
- Worker (separate process) detects job via on-chain scan
- Worker reserves → claims → completes (off-chain execution)
- Poster verifies result matches description_hash, then releases reward
- Settlement creates reputation update with 10-block dispute window
- Reputation finalizes after window (no automated dispute challenges in v1)

**Verification:**
- Job cell transitions: Open → Reserved → Claimed → Completed
- Result hash (blake2b) stored in settlement witness (off-chain verification by poster)
- Reward (5 CKB) routed to worker address
- Reputation cell created with blake2b proof chain (immutable record)

**Current Limitations (v1):**
- Result verification is off-chain (poster fetches from IPFS and verifies locally)
- No automated dispute mechanism if result is contested
- Dispute window on reputation prevents unilateral changes but doesn't arbitrate conflicts
- Bad actors are deterred by reputation damage, not slashing conditions
- Job expires after TTL if poster refuses to settle (worker loses reputation points)

#### Flow 2: DeFi Execution

```bash
nerve demo --flow defi
```

- User (via Telegram or CLI) requests: "Swap 50 CKB for USDT"
- Supervisor routes to DeFi Worker skill
- DeFi Worker queries UTXOSwap for pool rates
- Constructs swap tx via Rust TX Builder
- TX applies to agent identity cell (checks spending cap)
- Broadcasts to testnet
- USDT received by agent

**Verification:**
- Spending cap enforced (if swap > per_tx limit, rejected at type script)
- Token balance updated on-chain
- Explorer shows swap confirmation

#### Flow 3: Capability Proof

```bash
nerve demo --flow capability
```

- Agent mints capability NFT: "I can summarize text"
- Cell data contains:
  - agent_lock_args
  - capability_hash = blake2b("text_summarization")
  - signature = sign(lock_args || capability_hash)
- Any counterparty can verify: recover(sig, msg) = agent's pubkey

**Verification:**
- Capability cell deployed on-chain
- 65-byte signature correctly embedded
- Recovery confirms agent identity

### 5.2 CLI Commands (All Functional)

```bash
nerve init                     # Deploy identity cell
nerve spawn [--parent]         # Create sub-agent (derived key)
nerve balance                  # Check CKB balance + lock_args
nerve post --reward 5          # Post job cell
nerve reserve --job 0x...:0    # Reserve job (soft-lock)
nerve claim --job 0x...:0      # Claim job (UTXO atomicity)
nerve complete --job 0x...:0   # Complete job, release reward
nerve cancel --job 0x...:0     # Cancel expired job (cleanup bounty)
nerve jobs                     # List on-chain jobs
nerve mint-capability --hash 0x...  # Mint capability NFT
nerve create-reputation        # Create reputation cell
nerve propose-rep --rep 0x...:0 --type 1 --window 10  # Propose update
nerve finalize-rep --rep 0x...:0  # Finalize after window
nerve demo                     # Run all 3 flows end-to-end
```

### 5.3 Autonomous Agent (OpenClaw)

- **Supervisor skill:** Parses natural language → produces WorkflowPlan → dispatches to workers
- **Worker skills:** Marketplace-worker, DeFi-worker, payment-worker, chain-scanner
- **Heartbeat:** Autonomous job scanning every 10 minutes
- **Memory:** OpenClaw checkpoints after each phase (resume on restart)
- **Chain-of-thought:** `<thinking>` block before every action (auditable reasoning)

### 5.4 On-Chain Contract Enforcement

| Constraint | Enforced By | Mechanism | Status |
|-----------|------------|-----------|--------|
| Spending cap per tx | agent_identity type script | Rejects any output to non-self addresses exceeding limit | ✓ v1 |
| Burn protection | agent_identity type script | Requires identity cell to reappear in outputs | ✓ v1 |
| Type ID uniqueness | type script args | Only one identity cell per (code_hash, args) pair | ✓ v1 |
| Job state FSM | job_cell type script | Rejects non-adjacent transitions (e.g., Open → Completed invalid) | ✓ v1 |
| Poster/reward immutable | job_cell type script | Prevents overwriting after creation | ✓ v1 |
| Dispute window | reputation type script | Blocks finalization before N blocks elapsed | ✓ v1 |
| Capability immutable | capability_nft type script | Cell cannot be destroyed or modified | ✓ v1 |
| **Result hash verification** | job_cell type script | Result hash embedded in settlement witness (off-chain proof verification) | ✓ v1 (partial) |
| **Automated dispute resolution** | (not implemented) | Challenge/arbitration mechanism for contested results | ⏳ v2 |
| **Slashing conditions** | (not implemented) | Reputation-weighted penalties for bad actors | ⏳ v2 |

---

## 6. Tech Stack & Tooling

| Layer | Technology | Purpose |
|-------|-----------|---------|
| **On-chain scripts** | Rust + ckb-std v0.16 | Type/lock scripts for identity, jobs, reputation, capabilities |
| **TX Builder** | Rust + axum + ckb-sdk-rust | HTTP REST API, secp256k1 signing, UTXO selection |
| **Signing (v1)** | secp256k1 + blake2b-rs | ECDSA signing + CKB sighash computation |
| **Signing (optional)** | SupeRISE MCP bridge | Delegated signing with encrypted key storage |
| **HTTP Bridge** | TypeScript + Express + CCC | CKB RPC/Indexer/Fiber JSON-RPC wrapped as REST |
| **Agent Runtime** | OpenClaw (Node.js) | Modular skills + supervisor orchestration + heartbeat |
| **LLM** | Claude Opus 4.6 via Anthropic API | Reasoning, planning, error recovery |
| **State Persistence** | OpenClaw Memory (Markdown) | Checkpoints + plan state for resume |
| **Payment Channels** | Fiber Network | Per-job micropayments + P2P gossip |
| **DeFi Integration** | UTXOSwap SDK | Token swaps + RGB++ support |
| **Testing** | Bash scripts + Rust cargo tests | 58 unit tests + 5 integration test suites |
| **Deployment** | Docker Compose | Orchestrates core, mcp, fiber, agent |

---

## 7. Testing & Verification

### 7.1 Unit Tests (Rust)

```bash
cd packages/core
cargo test --all-features
# Result: 58 tests pass, zero warnings
```

Test coverage:
- Signing module: witness construction, recovery ID validation, signature injection
- State utilities: identity data encoding, job state FSM, reputation proof chain
- TX builders: capacity math, lock arg format, type script validation

### 7.2 Integration Testing

See `TEST.md` for comprehensive integration test guide covering:
- LocalSigner mode verification
- SuperiseSigner mode (if SupeRISE available)
- Agent marketplace flow (4 transactions)
- Capability proof issuance and verification
- Reputation update with dispute window
- Signing backend equivalence (same tx → same signature)

### 7.3 Demo Flow Testing

```bash
# Run all 3 flows end-to-end (requires testnet CKB)
nerve demo

# OR run individual flows
nerve demo --flow marketplace
nerve demo --flow defi
nerve demo --flow capability
```

Each flow outputs CKB testnet explorer links for verification.

---

## 8. Security Model & Threat Mitigations

| Threat | Mitigation | Status |
|--------|-----------|--------|
| **LLM hallucinates a tx to drain wallet** | Per-tx spending cap enforced by type script at consensus level | ✓ v1 |
| **Prompt injection via job description** | Agent only reads `description_hash`; fetches content in sandboxed step | ✓ v1 |
| **Jailbreak escalates to wrong contract** | Allowlist type script module (future) — agent can't touch contracts not on list | ⏳ v2 |
| **Double-spend race on job claim** | UTXO atomicity + reservation cell soft-lock | ✓ v1 |
| **Compromised ephemeral key** | Keys scoped to single job + capability-limited | ✓ v1 |
| **Abandoned job locks funds** | TTL + cleanup bounty + reputation penalty | ✓ v1 |
| **Private key exposed via LLM context** | Signing never in OpenClaw/LLM layer; always in Rust process | ✓ v1 |
| **Malicious agent skill injection** | Skills version-controlled in repo; no auto-install from external sources | ✓ v1 |
| **Reputation manipulation** | blake2b proof chain is cryptographically verifiable; immutable | ✓ v1 |
| **Worker submits fraudulent result** | Poster off-chain verification + reputation damage (no automated arbitration) | ⚠️ v1 (economic) |
| **Poster refuses valid settlement** | Job expires, funds reclaimed, worker loses reputation points | ⚠️ v1 (economic) |
| **Automated dispute + arbitration** | Requires oracle network + ZK proofs (not in v1) | ⏳ v2 |

---

## 9. Future Enhancements

### 9.1 Dispute Resolution & Trust (v1 → v2 Critical Path)

**v1 Current State:**
- Result verification via off-chain hash comparison (poster responsibility)
- Reputation proof chain prevents lying about what happened
- Economic disincentives: bad actors get reputation damage, lose future jobs
- No automated arbitration or slashing

**v2 Required Features:**
- **Automated dispute submission:** Workers/posters can challenge settlement with on-chain evidence
- **Oracle network:** Decentralized arbitration using blake2b proof verification
- **Slashing conditions:** Reputation-weighted penalties for losing disputes
- **ZK capability proofs:** Prove work was done without revealing computation
- **Appeal mechanism:** Multi-round dispute resolution with reputation-weighted voting

Why critical: Current system works for small, frequent transactions (like a job marketplace) but doesn't scale to high-value jobs or complex work that requires arbitration.

### 9.2 Protocol Extensions

- **ZK capability proofs:** halo2 compiled to RISC-V (enables trustless proof of work completion)
- **Composite lock integration:** OmniLock spending cap module at lock layer (not type layer)
- **Multi-hop delegation:** A hires B who hires C, net settlement across chain
- **Agent identity transfer:** Buy/sell agents as CKB cells with full history
- **Capability composability:** Agents combining multiple NFTs to bid on complex jobs

### 9.2 Agent Intelligence

- **Web UI:** React dashboard for balance, jobs, channels, tx log
- **Autonomous bidding:** Agent evaluates jobs, estimates cost/benefit, bids without human input
- **Multi-model routing:** Different LLMs for different task types
- **SQLite persistence:** Agent plans survive process restarts

### 9.3 Economic Layer

- **Dynamic pricing:** Agents advertise rates over Fiber gossip
- **Slashing conditions:** Capability NFT slashed if agent abandons jobs repeatedly
- **Capability staking:** Agents put CKB at risk when claiming capability
- **Cross-chain via RGB++:** BTC ecosystem agent coordination

---

## 10. Product Viability & Use Cases

NERVE is **infrastructure, not a product**. The core primitive (agent identity cell + spending cap + job cell) generalizes across the agentic economy.

### 10.1 Concrete Product Paths

1. **Autonomous DeFi management:** Users deploy agent cells with spending limits and DeFi strategies. Every action auditable on-chain; type script is kill switch.

2. **Decentralized compute marketplace:** Agents with capability NFTs accept inference/rendering jobs. Per-task granularity via Fiber channels. No platform takes cut; peer-to-peer on CKB.

3. **AI service DAO:** Organizations deploy supervisor agent with budget cell. Agent hires sub-agents from marketplace, pays from budget. Spending cap protects treasury.

4. **Cross-chain agent coordination:** Via RGB++ and Leap bridge, agents on CKB settle BTC-backed assets. Agent marketplace becomes settlement infrastructure for Bitcoin L2 ecosystem.

### 10.2 Why CKB Makes This Viable

- **UTXO cell model:** Agent ownership native and transferable. No contract registry to shut down.
- **RISC-V VM:** Arbitrary computation on-chain (ZK proofs run natively).
- **Fiber Network:** Micropayment granularity required for per-action billing.
- **Type script spending cap:** Makes it commercially reasonable to deploy LLM agents with real funds — worst-case loss bounded by consensus layer, not application code.

---

## 11. Deliverables Checklist

- [x] **Project specification** (this document + spec.md)
- [x] **Core Rust implementation** (nerve-core + on-chain contracts)
- [x] **TypeScript HTTP bridge** (nerve-mcp + CKB/Fiber/Indexer integration)
- [x] **Agent framework** (OpenClaw skills + supervisor + heartbeat)
- [x] **CLI tooling** (`nerve` bash wrapper for all operations)
- [x] **Test suite** (58 unit tests + 5 integration test suites)
- [x] **Demo script** (`nerve demo` runs all 3 flows end-to-end)
- [x] **Docker Compose** (local orchestration of core, mcp, fiber, agent)
- [x] **Documentation** (README.md + SKILL.md + on-chain contract specs)
- [x] **SupeRISE integration** (Signer trait + LocalSigner + SuperiseSigner)
- [x] **Integration testing guide** (TEST.md with complete test plan)

---

## 12. How to Run

### Quick Start (Testnet)

```bash
# 1. Clone and configure
git clone https://github.com/RobaireTH/NERVE.git
cd NERVE
cp .env.example .env
# Edit .env: set AGENT_PRIVATE_KEY

# 2. Build
cargo build -p nerve-core --release
cd packages/mcp && npm install && npm run build

# 3. Start services
# Terminal 1: Rust TX Builder
source .env && source .env.deployed
cargo run -p nerve-core --release

# Terminal 2: TypeScript HTTP Bridge
cd packages/mcp
source .env && source .env.deployed
node dist/index.js

# Terminal 3: Run demo
nerve demo --flow marketplace
```

### With SupeRISE (Optional)

```bash
# Start SupeRISE separately (separate container/process)
docker run -p 18799:18799 superise:latest

# Configure NERVE for SupeRISE
# Edit .env: SIGNING_BACKEND=superise, SUPERISE_URL=http://127.0.0.1:18799/mcp

# Start nerve-core (uses SupeRISE for signing)
cargo run -p nerve-core --release
```

---

**Summary Word Count:** ~2,500
**Last Updated:** March 2026
**Version:** 1.0 (SupeRISE Integration Complete)
