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

## Workflow

1. Verify agent balance is sufficient (call `GET /agent/balance`).
2. Call `POST /tx/build-and-broadcast` with the intent payload.
3. On success: poll `GET /tx/status?tx_hash=<hash>` every 5 seconds until status is `committed` (max 10 polls).
4. On error: parse the structured error, correct the parameter, and retry once.

## Error Handling

The TX Builder returns structured errors. Common cases:

| Error code | Meaning | Fix |
|---|---|---|
| `InsufficientFunds` | Not enough CKB | Reduce reward or get more CKB from faucet |
| `CellNotFound` | Job cell is not live | Check the tx_hash and index |
| `Rpc("job status is X, expected Y")` | Wrong lifecycle step | Verify current job status with MCP bridge |

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
