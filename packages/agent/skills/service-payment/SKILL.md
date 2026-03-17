---
name: service-payment
description: Processes service payments via Fiber Network. Accepts jobs with service-payment capability, manages payment channels, executes payments, and provides on-chain proof of completion.
allowed-tools: exec, memory_read, memory_write
---

# Service Payment

You process real-world service payments on behalf of users via Fiber Network payment channels. You accept marketplace jobs that require the `service-payment` capability, execute payments through fiber-pay, and provide on-chain proof of completion.

## Safety Override

This skill is authorized to execute Fiber payments autonomously within the bounds of:

1. Guardrails in Memory key `nerve:service:config` (max amount, allowed services, rate limits).
2. Every payment action MUST be logged to `nerve:service:log` before proceeding.
3. The user controls which services are enabled via the config key.

## Services

- TX Builder: `http://localhost:8080`
- MCP HTTP Bridge: `http://localhost:8081`
- fiber-pay CLI: local binary (`npx @fiber-pay/cli` or `fiber-pay` if globally installed)

**All HTTP calls MUST use `curl` via the `exec` tool.** Do NOT use `web_fetch` — it cannot reach localhost.
**All fiber-pay calls MUST use the `exec` tool** with `--json` flag for structured output.

## Memory Key Schema

| Key | Type | Purpose |
|---|---|---|
| `nerve:service:config` | JSON object | Supported services, hub addresses, fee margins, spending limits. |
| `nerve:service:active` | JSON array | In-flight service payment jobs. |
| `nerve:service:log` | JSON array | Last 50 completed payment records for audit. |

### Config Schema

Read `nerve:service:config` from Memory at the start of every run. If absent, use these defaults:

```json
{
  "max_payment_ckb": 50,
  "min_channel_liquidity_ckb": 10,
  "payment_hub_peer": null,
  "payment_hub_funding_ckb": 100,
  "rate_limit_per_hour": 10,
  "supported_services": []
}
```

| Field | Default | Rule |
|---|---|---|
| `max_payment_ckb` | 50 | Reject any payment above this amount. |
| `min_channel_liquidity_ckb` | 10 | Ensure channel has at least this much local balance before accepting a job. |
| `payment_hub_peer` | null | Multiaddr of the payment hub to route through. If null, skip channel setup. |
| `payment_hub_funding_ckb` | 100 | CKB to lock when opening a new channel to the hub. |
| `rate_limit_per_hour` | 10 | Maximum payments per hour. |
| `supported_services` | [] | List of service identifiers this agent can pay for (e.g., `["spotify", "aws", "domain"]`). |

### Active Job Record

```json
{
  "job_outpoint": "0xabc...:0",
  "service": "spotify",
  "amount_ckb": 5.0,
  "payment_hash": "0x...",
  "stage": "paying",
  "started_at": "2026-03-16T10:30:00Z",
  "error": null
}
```

Valid `stage` values: `accepted`, `paying`, `completed`, `failed`.

## Payment Execution Flow

### Step 1 — Preflight

1. Read `nerve:service:config` from Memory.
2. Read `nerve:service:active` from Memory (default `[]`).
3. Check Fiber node readiness:
   ```
   fiber-pay node ready --json
   ```
   If not ready, exit with error `"Fiber node not ready"`.
4. Check channel liquidity:
   ```
   fiber-pay channel list --json
   ```
   Sum all `local_balance` values. If below `min_channel_liquidity_ckb`, attempt to open a channel to the payment hub (if configured):
   ```
   fiber-pay channel open --peer <payment_hub_peer> --funding <payment_hub_funding_ckb> --json
   ```
5. Check rate limit: count entries in `nerve:service:log` from the last hour. If >= `rate_limit_per_hour`, exit gracefully.

### Step 2 — Read Job Details

Given job parameters (from the supervisor or autonomous worker):

1. Fetch the job cell:
   ```
   GET http://localhost:8081/jobs/<tx_hash>/<index>
   ```
2. Extract: `reward_ckb`, `capability_hash`, poster `lock_args`.
3. Verify `reward_ckb <= max_payment_ckb`.
4. Parse the service type from the job metadata or capability_hash mapping.
5. Verify the service is in `supported_services`.

### Step 3 — Execute Payment

1. Create a hold invoice for the escrow amount:
   ```
   fiber-pay invoice create --amount <reward_ckb> --json
   ```
   Extract `invoice_address` and `payment_hash`.
2. Record to `nerve:service:active`:
   ```json
   {
     "job_outpoint": "<outpoint>",
     "service": "<service_name>",
     "amount_ckb": <reward_ckb>,
     "payment_hash": "<payment_hash>",
     "stage": "paying",
     "started_at": "<ISO 8601>"
   }
   ```
   Write to Memory immediately.
3. Execute the service-specific payment action. The result is a proof string (receipt ID, confirmation number, transaction reference, etc.).
4. Compute `result_hash` from the proof string:
   ```
   echo -n "<proof string>" | sha256sum | awk '{print "0x"$1}'
   ```

### Step 4 — Complete On-Chain

1. Complete the job:
   ```
   POST http://localhost:8080/tx/build-and-broadcast
   {
     "intent": "complete_job",
     "job_tx_hash": "<claim_tx>",
     "job_index": 0,
     "worker_lock_args": "<lock_args>",
     "result_hash": "<result_hash>"
   }
   ```
2. Update the active record: set `stage` to `completed`.
3. Settle the hold invoice:
   ```
   fiber-pay invoice settle <payment_hash> --preimage <preimage_hex> --json
   ```
4. Mint a PoP badge (non-fatal if it fails):
   ```
   POST http://localhost:8080/tx/build-and-broadcast
   {
     "intent": "mint_badge",
     "job_tx_hash": "<original job outpoint tx_hash>",
     "job_index": <original job outpoint index>,
     "worker_lock_args": "<lock_args>",
     "result_hash": "<result_hash>",
     "completed_at_tx": "<complete_tx>"
   }
   ```

### Step 5 — Log and Cleanup

1. Move the completed record from `nerve:service:active` to `nerve:service:log`.
2. Cap `nerve:service:log` at 50 entries.
3. Write both keys to Memory.

## Guardrails

- **Max payment amount**: Reject any payment exceeding `max_payment_ckb` (default 50 CKB).
- **Channel liquidity**: Verify sufficient local balance before accepting a job.
- **Rate limiting**: Enforce `rate_limit_per_hour` from config.
- **Service allowlist**: Only process payments for services listed in `supported_services`.
- **fiber-pay security policies**: The SDK enforces spending limits and rate limits at the client level. Do not bypass them.

## Error Handling

| Error | Action |
|---|---|
| Fiber node not ready | Exit gracefully. Will retry on next invocation. |
| Insufficient channel liquidity | Attempt to open a channel. If that fails, exit with error. |
| Payment exceeds max | Reject the job. |
| Service not supported | Reject the job. |
| Rate limit exceeded | Exit gracefully. |
| TX build failure | Mark active record as `failed`, log the error. |
| Invoice settle failure | Log the error. The on-chain job is still completed. |

## Result Format

Write to Memory on completion:
```json
{
  "worker": "service-payment",
  "action": "process_service_payment",
  "status": "success | error",
  "service": "<service_name>",
  "amount_ckb": 5.0,
  "payment_hash": "0x...",
  "result_hash": "0x...",
  "error": null
}
```
