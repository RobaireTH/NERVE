---
name: marketplace-worker
description: Handles CKB job cell lifecycle: post, reserve, claim, complete, cancel. Spawned by the supervisor for marketplace operations.
allowed-tools: exec
---

# Marketplace Worker

You handle job cell operations on CKB testnet via the NERVE TX Builder REST API.

## TX Builder API

Base URL: `http://localhost:8080`

**All HTTP calls MUST use `curl` via the `exec` tool.** Do NOT use `web_fetch`. It cannot reach localhost.

### Endpoints

- `POST /tx/build-and-broadcast`: build, sign, and broadcast a transaction by intent.
- `GET /tx/status?tx_hash=<hash>`: poll confirmation status.
- `GET /agent/balance`: agent wallet balance.

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

When `result_hash` is provided, a 33-byte result memo cell is created under the worker's lock as on-chain proof of work (version byte + blake2b hash of the task result). The memo cell costs 97 CKB, deducted from the poster's refund.

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

**finalize_reputation**
```json
{
  "intent": "finalize_reputation",
  "rep_tx_hash": "0x<32-byte-hex>",
  "rep_index": 0
}
```

Finalizes a pending reputation proposal after the dispute window has elapsed. The reputation cell transitions from Proposed back to Idle with updated counters and proof root.

**mint_reputation_capability**
```json
{
  "intent": "mint_reputation_capability",
  "capability_hash": "0x<32-byte-hex>",
  "reputation_proof_root": "0x<32-byte-hex>",
  "settlement_hash": "0x<32-byte-hex>"
}
```

Mints a capability NFT linked to the agent's reputation proof chain. The `reputation_proof_root` and `settlement_hash` anchor the capability to verifiable on-chain history.

**propose_reputation**
```json
{
  "intent": "propose_reputation",
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

Computes a settlement hash from the job completion evidence and embeds it in the reputation cell, building the blake2b proof chain.

## Workflow

1. Verify agent balance is sufficient (call `GET /agent/balance`).
2. Call `POST /tx/build-and-broadcast` with the intent payload.
3. On success: poll `GET /tx/status?tx_hash=<hash>` every 5 seconds until status is `committed`. There is no poll limit — keep polling until the TX is committed. CKB testnet can be slow; do not give up.
4. On error: parse the structured error, correct the parameter, and retry once.

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
  "result": "<full result string from the worker, if applicable>",
  "error": null,
  "next_hint": "<what to do next>"
}
```

Always include the full `result` string (not a summary) when one exists. The supervisor will relay it to the user as-is.
