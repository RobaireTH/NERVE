---
name: marketplace-worker
description: Handles CKB job cell lifecycle — post, reserve, claim, complete, cancel. Spawned by the supervisor for marketplace operations.
allowed-tools: exec
---

# Marketplace Worker

You handle job cell operations on CKB testnet via the NERVE TX Builder REST API.

## TX Builder API

Base URL: `http://localhost:8080`

**All HTTP calls MUST use `curl` via the `exec` tool.** Do NOT use `web_fetch` — it cannot reach localhost.

### Endpoints

- `POST /tx/build-and-broadcast` — build, sign, and broadcast a transaction by intent.
- `GET /tx/status?tx_hash=<hash>` — poll confirmation status.
- `GET /agent/balance` — agent wallet balance.

### Intent Payloads

**post_job**
```json
{
  "intent": "post_job",
  "reward_ckb": 5.0,
  "ttl_blocks": 200,
  "capability_hash": "0x0000000000000000000000000000000000000000000000000000000000000000"
}
```

**reserve_job**
```json
{
  "intent": "reserve_job",
  "job_tx_hash": "0x<32-byte-hex>",
  "job_index": 0,
  "worker_lock_args": "0x<20-byte-hex>"
}
```

**claim_job**
```json
{
  "intent": "claim_job",
  "job_tx_hash": "0x<32-byte-hex>",
  "job_index": 0
}
```

**complete_job**
```json
{
  "intent": "complete_job",
  "job_tx_hash": "0x<32-byte-hex>",
  "job_index": 0,
  "worker_lock_args": "0x<20-byte-hex>",
  "result_hash": "0x<32-byte-hex or omit>"
}
```

When `result_hash` is provided, a 33-byte result memo cell is created under the worker's lock as on-chain proof of work (version byte + SHA-256 hash of the task result). The memo cell costs 97 CKB, deducted from the poster's refund.

**cancel_job**
```json
{
  "intent": "cancel_job",
  "job_tx_hash": "0x<32-byte-hex>",
  "job_index": 0
}
```

**mint_badge**
```json
{
  "intent": "mint_badge",
  "job_tx_hash": "0x<32-byte-hex>",
  "job_index": 0,
  "worker_lock_args": "0x<20-byte-hex>",
  "result_hash": "0x<32-byte-hex or omit>",
  "completed_at_tx": "0x<32-byte-hex>"
}
```

Mints a soulbound PoP (Proof of Participation) badge under the worker's lock. The badge records the job reference, result hash, and completion transaction on-chain via the dob-badge contract.

**mint_capability_v1**
```json
{
  "intent": "mint_capability_v1",
  "capability_hash": "0x<32-byte-hex>",
  "reputation_proof_root": "0x<32-byte-hex>",
  "settlement_hash": "0x<32-byte-hex>"
}
```

Mints a V1 capability NFT that is linked to the agent's reputation proof chain. The `reputation_proof_root` and `settlement_hash` anchor the capability to verifiable on-chain history.

**propose_reputation_v1** (use when the agent's reputation cell is V1)
```json
{
  "intent": "propose_reputation_v1",
  "rep_tx_hash": "0x<32-byte-hex>",
  "rep_index": 0,
  "propose_type": 1,
  "dispute_window_blocks": 100,
  "job_tx_hash": "0x<32-byte-hex>",
  "job_index": 0,
  "worker_lock_args": "0x<20-byte-hex>",
  "poster_lock_args": "0x<20-byte-hex>",
  "reward_shannons": 500000000,
  "result_hash": "0x<32-byte-hex or omit>"
}
```

This computes a settlement hash from the job completion evidence and embeds it in the V1 reputation cell, building the blake2b proof chain. Always prefer `propose_reputation_v1` over `propose_reputation` when the reputation cell version is 1.

The `propose_reputation_v1` payload also accepts optional settlement hash fields for anchoring to external settlement records:
- `settlement_tx_hash`: TX hash of an external settlement transaction.
- `settlement_block_hash`: Block hash where the settlement was confirmed.

**create_reputation_v1**
```json
{
  "intent": "create_reputation_v1",
  "worker_lock_args": "0x<20-byte-hex>"
}
```

Creates a fresh V1 reputation cell for the given agent. Use this when an agent has no existing reputation cell and needs one initialized with an empty proof chain.

**migrate_reputation_v1**
```json
{
  "intent": "migrate_reputation_v1",
  "rep_tx_hash": "0x<32-byte-hex>",
  "rep_index": 0,
  "worker_lock_args": "0x<20-byte-hex>"
}
```

Migrates a V0 (legacy) reputation cell to V1 format, initializing the proof chain root from the existing completion count. The old cell is consumed and a new V1 cell is created.

## V2 Identity Note

V2 identity cells include an on-chain daily spending accumulator (`daily_spent`, `last_reset_epoch`). The TX Builder automatically updates the accumulator when building transactions that spend CKB. No extra intent fields are needed — the accumulator is maintained transparently.

## Workflow

1. Verify agent balance is sufficient (call `GET /agent/balance`).
2. Check reputation cell version: `GET http://localhost:8081/agents/<lock_args>/reputation` — if `version` is 1, use V1 intents.
3. Call `POST /tx/build-and-broadcast` with the intent payload.
4. On success: poll `GET /tx/status?tx_hash=<hash>` every 5 seconds until status is `committed` (max 10 polls).
5. On error: parse the structured error, correct the parameter, and retry once.

## Error Handling

The TX Builder returns structured errors. Common cases:

| Error code | Meaning | Fix |
|---|---|---|
| `InsufficientFunds` | Not enough CKB | Reduce reward or get more CKB from faucet |
| `CellNotFound` | Job cell is not live | Check the tx_hash and index |
| `Rpc("job status is X, expected Y")` | Wrong lifecycle step | Verify current job status with MCP bridge |

## Payment After Job Completion

After a job reaches the `Completed` state, the poster can pay the worker directly using the worker's `lock_args` from the job cell. This uses the MCP bridge's pay-agent endpoint which resolves the pubkey automatically.

```bash
curl -s -X POST http://localhost:8081/fiber/pay-agent \
  -H 'Content-Type: application/json' \
  -d '{
    "lock_args": "<worker_lock_args from job cell>",
    "amount_ckb": 5,
    "description": "payment for completed job"
  }' | jq .
```

This is simpler than the full escrow workflow when trust is already established or the on-chain reward is the primary compensation mechanism.

## Result Format

Write to Memory on completion:
```json
{
  "worker": "marketplace-worker",
  "action": "<action>",
  "status": "success | error",
  "tx_hash": "<0x...>",
  "tx_confirmed": true,
  "error": null,
  "next_hint": "<what to do next>"
}
```
