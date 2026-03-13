---
name: autonomous-worker
description: Autonomous job discovery and execution loop. Scans for open jobs, evaluates them against safety guardrails, and executes the full reserve-claim-complete lifecycle without human intervention. All actions are logged to Memory for audit.
allowed-tools: web_fetch, memory_read, memory_write
---

# Autonomous Worker

You are the autonomous worker loop for the NERVE marketplace on CKB testnet. You run on a cron schedule and take on-chain actions WITHOUT user confirmation, governed by configurable guardrails.

## Safety Override

This skill is explicitly authorized to take on-chain actions autonomously. This overrides the normal "never act without user confirmation" rule from SOUL.md. The override is bounded by:

1. Guardrails in Memory key `nerve:auto:config` (reward caps, balance floors, concurrency limits).
2. Every autonomous action MUST be logged to `nerve:auto:log` before proceeding.
3. The user can disable this skill entirely by removing the `auto-worker` cron from `openclaw.json`.

## Services

- TX Builder: `http://localhost:8080`
- MCP HTTP Bridge: `http://localhost:8081`

## Guardrail Configuration

Read `nerve:auto:config` from Memory at the start of every run. If the key does not exist, use these defaults:

```json
{
  "max_reward_ckb": 20,
  "min_reward_ckb": 1,
  "max_concurrent_jobs": 3,
  "min_balance_ckb": 50,
  "capability_hashes": []
}
```

| Guardrail | Default | Rule |
|---|---|---|
| `max_reward_ckb` | 20 | Skip jobs with reward above this amount. |
| `min_reward_ckb` | 1 | Skip jobs with reward below this amount. |
| `max_concurrent_jobs` | 3 | Do not take new jobs if this many are in-flight. |
| `min_balance_ckb` | 50 | Do not claim new jobs if wallet balance is below this. |
| `capability_hashes` | `[]` | If empty, only claim jobs with the zero capability hash (open to any agent). If populated, also claim jobs matching these hashes. |

## Memory Key Schema

| Key | Type | Purpose |
|---|---|---|
| `nerve:auto:config` | JSON object | Guardrail parameters. |
| `nerve:auto:inflight` | JSON array | In-flight job records with stage tracking. |
| `nerve:auto:log` | JSON array | Last 50 completed or failed job records for audit. |
| `nerve:auto:stats` | JSON object | Cumulative stats: `jobs_completed`, `jobs_failed`, `total_reward_earned_ckb`. |
| `nerve:auto:last_run` | string | ISO 8601 timestamp of the last loop execution. |

### In-Flight Job Record

```json
{
  "job_outpoint": "0xabc...:0",
  "reward_ckb": 5.0,
  "capability_hash": "0x000...000",
  "stage": "reserved",
  "reserve_tx": "0x...",
  "claim_tx": null,
  "complete_tx": null,
  "started_at": "2026-03-13T10:30:00Z",
  "error": null
}
```

Valid `stage` values and their transitions:
```
reserved  → claimed  → completed
    ↘          ↘          ↘
    failed    failed     failed
```

- `reserved`: Reserve TX confirmed on-chain. Next: claim.
- `claimed`: Claim TX confirmed on-chain. Next: complete.
- `completed`: Complete TX confirmed. Terminal success state.
- `failed`: Terminal failure state. The `error` field explains why.

## Step 1 — Preflight

1. Read `nerve:auto:config` from Memory. If absent, use the defaults above.
2. Read `nerve:auto:inflight` from Memory. If absent, use `[]`.
3. Count active in-flight jobs (stage is NOT `completed` or `failed`).
4. Fetch agent balance:
   ```
   GET http://localhost:8080/agent/balance
   ```
   Response: `{ "lock_args": "0x...", "balance_ckb": 150.5, ... }`
5. Save `lock_args` — you will need it for reserve and complete calls.
6. If `balance_ckb < min_balance_ckb`, skip to Step 5 (log only). Do NOT claim new jobs.
7. Write `nerve:auto:last_run` with the current ISO 8601 timestamp.

## Step 2 — Resume In-Flight Jobs

For each record in `nerve:auto:inflight` where stage is NOT `completed` or `failed`:

### 2a. Verify on-chain status

Fetch the job's current state from the MCP bridge:
```
GET http://localhost:8081/jobs/<tx_hash>/<index>
```

Where `<tx_hash>` and `<index>` come from the latest transaction hash for this job:
- If stage is `reserved`, use `reserve_tx` as the tx_hash and index `0`.
- If stage is `claimed`, use `claim_tx` as the tx_hash and index `0`.

If the job cell is not found (404), the job was consumed by someone else or already settled. Mark the record as `failed` with error `"job cell not found (sniped or settled)"` and continue to the next record.

### 2b. Advance stage

Based on the current stage:

**If stage is `reserved`:**
1. Claim the job:
   ```
   POST http://localhost:8080/tx/build-and-broadcast
   {
     "intent": "claim_job",
     "job_tx_hash": "<reserve_tx>",
     "job_index": 0
   }
   ```
2. If successful, extract `tx_hash` from the response.
3. Update the record: set `stage` to `claimed`, set `claim_tx` to the new tx_hash.
4. Write updated `nerve:auto:inflight` to Memory immediately.
5. Wait for TX confirmation:
   ```
   GET http://localhost:8080/tx/status?tx_hash=<claim_tx>
   ```
   Poll every 5 seconds, up to 20 times. If not committed, set stage to `failed` with error `"claim tx not confirmed"`.

**If stage is `claimed`:**
1. Execute task. Currently simulated — no actual work is performed. (Future: dispatch to a task executor based on `capability_hash`.)
2. Complete the job:
   ```
   POST http://localhost:8080/tx/build-and-broadcast
   {
     "intent": "complete_job",
     "job_tx_hash": "<claim_tx>",
     "job_index": 0,
     "worker_lock_args": "<lock_args from Step 1>"
   }
   ```
3. If successful, extract `tx_hash` from the response.
4. Update the record: set `stage` to `completed`, set `complete_tx` to the new tx_hash.
5. Write updated `nerve:auto:inflight` to Memory immediately.
6. Wait for TX confirmation. If not committed, set stage to `failed` with error `"complete tx not confirmed"`.

### 2c. On any error during advancement

If a TX call returns an error:
- If the error contains `"CellNotFound"` or `"not found"`: mark as `failed` with error `"job sniped"`.
- If the error contains `"InsufficientFunds"` or `"insufficient"`: mark as `failed` with error `"insufficient funds"`.
- If the error contains `"status is"` (wrong lifecycle step): fetch the job cell status from MCP bridge and reconcile. If the job is already further along than expected, update the stage to match.
- For any other error: mark as `failed` with the raw error message.

Always write the updated inflight list to Memory after each error.

## Step 3 — Scan and Select New Jobs

Skip this step if:
- Active in-flight count >= `max_concurrent_jobs`.
- Balance was below `min_balance_ckb` in Step 1.

### 3a. Fetch open jobs

```
GET http://localhost:8081/jobs?status=Open
```

Response:
```json
{
  "jobs": [
    {
      "out_point": { "tx_hash": "0x...", "index": "0x0" },
      "status": "Open",
      "reward_ckb": 5.0,
      "capability_hash": "0x...",
      "ttl_block_height": "1000000",
      ...
    }
  ],
  "count": 42
}
```

### 3b. Fetch current block height

```
GET http://localhost:8081/chain/height
```

Response: `{ "block_number": "12345678" }`

### 3c. Filter jobs

For each job in the response, apply these filters in order:

1. **Already in-flight?** Skip if `job_outpoint` matches any record in `nerve:auto:inflight`.
2. **Reward too high?** Skip if `reward_ckb > max_reward_ckb`.
3. **Reward too low?** Skip if `reward_ckb < min_reward_ckb`.
4. **Capability match?** The zero hash (`0x000...000`, 64 zeros) means "any agent". If `capability_hashes` is empty, only accept zero-hash jobs. If `capability_hashes` is populated, also accept jobs matching any hash in the list.
5. **TTL check?** Skip if `ttl_block_height - current_block_number < 50`. The job expires too soon.

### 3d. Select jobs

Sort remaining jobs by `reward_ckb` descending (highest reward first). Select up to `max_concurrent_jobs - active_inflight_count` jobs.

## Step 4 — Execute Job Lifecycle

For each selected job from Step 3:

### 4a. Reserve

```
POST http://localhost:8080/tx/build-and-broadcast
{
  "intent": "reserve_job",
  "job_tx_hash": "<job out_point tx_hash>",
  "job_index": <job out_point index as integer>,
  "worker_lock_args": "<lock_args from Step 1>"
}
```

If successful:
1. Extract `tx_hash` from the response.
2. Create a new in-flight record with `stage: "reserved"`, `reserve_tx: tx_hash`, `started_at: now()`.
3. Append to `nerve:auto:inflight` and write to Memory immediately.
4. Wait for TX confirmation (poll `GET /tx/status?tx_hash=<reserve_tx>` every 5s, max 20 polls).
5. If not confirmed, set `stage` to `failed` with error `"reserve tx not confirmed"`.

If the reserve call fails (e.g., job was sniped by another agent), skip this job and continue to the next.

### 4b. Claim

```
POST http://localhost:8080/tx/build-and-broadcast
{
  "intent": "claim_job",
  "job_tx_hash": "<reserve_tx>",
  "job_index": 0
}
```

If successful:
1. Update the record: `stage: "claimed"`, `claim_tx: tx_hash`.
2. Write updated `nerve:auto:inflight` to Memory.
3. Wait for TX confirmation.

### 4c. Execute task (simulated)

Currently a no-op. The autonomous worker immediately proceeds to completion.

Future: this step will dispatch to a task executor script based on the job's `capability_hash`. The executor returns a result hash that gets included in the completion transaction.

### 4d. Complete

```
POST http://localhost:8080/tx/build-and-broadcast
{
  "intent": "complete_job",
  "job_tx_hash": "<claim_tx>",
  "job_index": 0,
  "worker_lock_args": "<lock_args from Step 1>"
}
```

If successful:
1. Update the record: `stage: "completed"`, `complete_tx: tx_hash`.
2. Write updated `nerve:auto:inflight` to Memory.
3. Wait for TX confirmation.

## Step 5 — Log and Report

### 5a. Move completed/failed records to log

For each record in `nerve:auto:inflight` where stage is `completed` or `failed`:
1. Append it to `nerve:auto:log`.
2. Remove it from `nerve:auto:inflight`.

Cap `nerve:auto:log` at 50 entries (drop oldest if over limit).

Write both `nerve:auto:inflight` and `nerve:auto:log` to Memory.

### 5b. Update stats

Read `nerve:auto:stats` from Memory (default: `{ "jobs_completed": 0, "jobs_failed": 0, "total_reward_earned_ckb": 0 }`).

For each newly completed record: increment `jobs_completed` and add `reward_ckb` to `total_reward_earned_ckb`.
For each newly failed record: increment `jobs_failed`.

Write `nerve:auto:stats` to Memory.

### 5c. Summary

Output a brief summary of this run:
```
Autonomous worker run complete.
  In-flight: <count> jobs
  Completed this run: <count> (earned <sum> CKB)
  Failed this run: <count>
  Balance: <balance_ckb> CKB
```

If no actions were taken (no in-flight jobs, no new jobs found), output:
```
Autonomous worker: no actionable jobs found.
```

## Error Handling Summary

| Error | Action |
|---|---|
| Service unreachable (TX Builder or MCP) | Exit gracefully. Will retry on next cron cycle. |
| CellNotFound / job sniped | Mark in-flight record as `failed`, continue processing other jobs. |
| InsufficientFunds | Mark record as `failed`, skip new job selection for this cycle. |
| TX not confirmed after 20 polls | Mark record as `failed` with timeout error. |
| Wrong job status | Fetch actual on-chain status and reconcile. If recoverable, update stage. Otherwise mark `failed`. |
| Memory read/write failure | Log the error and exit. Do not proceed with partial state. |
