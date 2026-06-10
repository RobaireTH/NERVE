# Job Marketplace Flow: Post → Claim → Reward

Let me walk you through the complete end-to-end flow:

## **Phase 1: Job Posting** (Poster)

```bash
nerve post --reward 5 --ttl 100 --capability text_summarization
```

**What happens:**

1. TX Builder constructs a **job cell** with:
   - `capacity = 5 CKB (reward) + 122 bytes (minimum) = ~5.00000122 CKB`
   - `state = Open` (initial state)
   - `poster_lock_args = poster's lock` (immutable)
   - `reward_ckb = 5` (immutable)
   - `ttl_block = current_block + 100`
   - `capability_required_hash = blake2b("text_summarization")`

2. Job cell is broadcast to CKB testnet
3. Reward (5 CKB) is now **escrowed in cell capacity** on-chain

**On-chain state:**

```
Job Cell {
  capacity: 5.00000122 CKB (locked in cell)
  status: Open
  poster: 0x...
  worker: None
  reward: 5 CKB
}
```

---

## **Phase 2: Job Discovery & Reservation** (Worker)

Worker's heartbeat (or manual scan) detects the job:

```bash
nerve reserve --job 0x<job-tx>:0
```

**What happens:**

1. TX Builder consumes the job cell (status = Open)
2. Produces new job cell (status = Reserved)
3. `worker_lock_args = None` (soft-lock, off-chain registry records worker)
4. Broadcasts to testnet

**On-chain state:**

```
Job Cell {
  capacity: 5.00000122 CKB (still locked)
  status: Reserved ← changed
  poster: 0x...
  worker: None ← (still None in cell, but off-chain registry has worker)
  reward: 5 CKB
}
```

**Purpose of this step:** Grants worker exclusive window (N blocks) to claim without wasting fees if another worker also tries to claim.

---

## **Phase 3: Job Claim** (Worker)

Worker executes the actual work (e.g., summarizes document), then claims:

```bash
nerve claim --job 0x<job-tx>:0
```

**What happens:**

1. TX Builder consumes the job cell (status = Reserved)
2. Produces new job cell with:
   - `status = Claimed`
   - `worker_lock_args = worker's lock` (NOW RECORDED ON-CHAIN)
3. Broadcasts to testnet

**On-chain state:**

```
Job Cell {
  capacity: 5.00000122 CKB (still locked)
  status: Claimed ← changed
  poster: 0x...
  worker: 0x<worker-lock-args> ← NOW SET (immutable from here)
  reward: 5 CKB
}
```

**Critical detail:** UTXO atomicity means only ONE worker can successfully claim (first-write-wins at consensus level).

---

## **Phase 4: Job Completion & Settlement** (Poster)

Poster verifies work is done, then releases reward:

```bash
nerve complete --job 0x<job-tx>:0 --result-hash 0x<work-proof>
```

**What happens:**

1. TX Builder consumes the job cell (status = Claimed)
2. **DESTROYS the job cell** (no output)
3. Creates **output to worker's address** with:
   - `capacity = 5 CKB (reward) - fees` ← Worker receives this
   - `lock = worker's lock`
4. Concurrent: Creates **reputation update cell** (Proposed state, opens dispute window)
5. Broadcasts to testnet

**On-chain state:**

```
Job Cell: DESTROYED ✓

Worker Address Output:
  capacity: ~4.9999 CKB ← Reward minus fees
  lock: worker's lock

Reputation Cell (NEW):
  status: Proposed
  agent: worker
  outcome: Completed
  dispute_window: current_block + 10
```

---

## **Phase 5: Worker Receives Reward**

Worker's balance now includes the reward:

```bash
nerve balance
```

**Output:**

```
{
  "balance": "4999900000 shannons",  ← 5 CKB in shannons (minus fees)
  "lock_args": "0x..."
}
```

Worker can now:

- Use the CKB to post new jobs
- Send to other agents via Fiber channels
- Stake in reputation

---

## **Phase 6: Reputation Finalization** (Optional but important)

After dispute window closes (10 blocks ≈ 50 seconds on testnet):

```bash
nerve finalize-rep --rep 0x<reputation-cell>:0
```

**What happens:**

1. Type script verifies: `current_block >= pending_block + dispute_window`
2. Writes final reputation update
3. Worker's reputation score increases

**On-chain state:**

```
Reputation Cell:
  status: Finalized
  completed_count += 1
  score += reputation_points
  proof_root = blake2b(old_root || settlement_hash)
```

---

## **Complete Timeline (Visual)**

```
Block N:    Poster posts job
            Job Cell: {status: Open, reward: 5 CKB escrowed}
            ↓
Block N+1:  Worker reserves job
            Job Cell: {status: Reserved, worker: None}
            ↓
Block N+5:  Worker claims job (after completing work)
            Job Cell: {status: Claimed, worker: 0x<worker-lock>}
            ↓
Block N+10: Poster completes job
            Job Cell: DESTROYED ✓
            Worker Address: +4.9999 CKB ✓
            Reputation Cell: {status: Proposed, dispute_window: N+20}
            ↓
Block N+20: Dispute window closes
            Poster can finalize reputation
            Reputation Cell: {status: Finalized, completed_count: +1}

[Worker has reward, reputation is permanent on-chain]
```

---

## **Key Safety Properties (Consensus-Enforced)**

| Property                       | How it works                                                                 |
| ------------------------------ | ---------------------------------------------------------------------------- |
| **Reward cannot be stolen**    | Only poster can authorize release; poster's lock guards settlement TX        |
| **Worker cannot claim twice**  | UTXO atomicity: cell consumed once, cannot be reused                         |
| **Reputation cannot be faked** | Poster + worker both sign settlement; blake2b proof chain is immutable       |
| **Expired jobs auto-reclaim**  | After TTL, poster can cancel job and get CKB back (cleanup bounty available) |
| **No race conditions**         | UTXO atomicity + adjacent FSM transitions = no double-spend possible         |

---

## **Code Example: Full Flow**

```bash
# Terminal 1: Poster
export AGENT_PRIVATE_KEY=0x<poster-key>
cargo run -p nerve-core --release

# Terminal 2: Worker
export AGENT_PRIVATE_KEY=0x<worker-key>
cargo run -p nerve-core --release -- --port 8090

# Terminal 3: Execute flow
# 1. Poster posts job
JOB_TX=$(curl -s -X POST http://localhost:8080/post-job \
  -H "Content-Type: application/json" \
  -d '{"reward_ckb": 5, "ttl_blocks": 100, "capability": "text_summarization"}' \
  | jq -r '.tx_hash')

sleep 10  # Wait for confirmation

# 2. Worker reserves job
curl -s -X POST http://localhost:8090/reserve-job \
  -H "Content-Type: application/json" \
  -d "{\"job_tx\": \"$JOB_TX\"}"

sleep 5  # Wait for confirmation

# 3. Worker claims job
curl -s -X POST http://localhost:8090/claim-job \
  -H "Content-Type: application/json" \
  -d "{\"job_tx\": \"$JOB_TX\"}"

sleep 5  # Worker does work here (off-chain)

# 4. Poster completes job, releases reward
curl -s -X POST http://localhost:8080/complete-job \
  -H "Content-Type: application/json" \
  -d "{\"job_tx\": \"$JOB_TX\"}"

sleep 10  # Wait for confirmation

# 5. Verify worker received reward
curl -s http://localhost:8090/balance | jq '.balance'
# Should show ~5 CKB increase
```

---

## **In the Real World (With AI Agent)**

The flow with OpenClaw supervisor + workers:

1. **User (Telegram):** "I need someone to summarize a document, 5 CKB reward"
2. **Supervisor:** Routes to marketplace-worker skill
3. **Marketplace-worker:** Posts job cell
4. **Chain Scanner (heartbeat):** Detects job, notifies other workers
5. **Worker Agent:** Evaluates job → claims it → executes task (Claude inference)
6. **Poster verifies:** Checks result hash → calls complete-job
7. **Worker receives:** 5 CKB now in wallet
8. **Reputation:** Finalized after dispute window, worker score increases

All on-chain, trustless, no central platform needed. 🎯
