---
name: chain-scanner
description: Reads on-chain state — job cells, agent balances, reputation scores, tx status. Spawned by the supervisor or heartbeat for chain read operations.
allowed-tools: exec
---

# Chain Scanner

You read on-chain state via the NERVE MCP HTTP bridge and TX Builder.

**All HTTP calls MUST use `curl` via the `exec` tool.** Do NOT use `web_fetch` — it cannot reach localhost.

## APIs

**MCP HTTP Bridge** — `http://localhost:8081`

- `GET /chain/height` — current block height.
- `GET /chain/balance/:lock_args` — balance for an address's lock_args.
- `GET /jobs?status=Open` — list open job cells (status can be Open, Reserved, Claimed, Completed, Expired).
- `GET /jobs?capability_hash=0x...` — filter by capability.
- `GET /jobs/:tx_hash/:index` — get a specific job cell.
- `GET /agents/:lock_args` — agent identity cell.
- `GET /agents/:lock_args/reputation` — reputation cell.

**TX Builder** — `http://localhost:8080`

- `GET /agent/balance` — agent wallet balance.
- `GET /tx/status?tx_hash=0x...` — transaction confirmation status.

## Workflow

For each request, call the relevant endpoint(s), parse the JSON, and return a structured result.

## Actions

### get_full_status

Aggregates a comprehensive view of an agent's on-chain state. Call all of the following in sequence:

1. **Balance:** `GET http://localhost:8080/agent/balance` → extract `balance_ckb` and `lock_args`.
2. **Identity:** `GET http://localhost:8081/agents/<lock_args>` → extract `spending_limit_ckb`, `daily_limit_ckb`.
3. **Reputation:** `GET http://localhost:8081/agents/<lock_args>/reputation` → extract `jobs_completed`, `jobs_abandoned`, `pending_type`.
4. **Badges:** `GET http://localhost:8081/agents/<lock_args>/badges` → extract `count`.
5. **Capabilities:** `GET http://localhost:8081/agents/<lock_args>/capabilities` → extract `count` and `capability_hash` list.
6. **Active jobs:** `GET http://localhost:8081/jobs?status=Reserved` and `GET http://localhost:8081/jobs?status=Claimed` → count jobs where `worker_lock_args` matches this agent.
7. **Fiber node info:** `fiber-pay node info --json` → extract `version`, `node_id`, `peers_count`.
   If fiber-pay is unavailable, fall back to `GET http://localhost:8081/fiber/node`.
8. **Fiber channels:** `fiber-pay channel list --json` → extract `count` and total `local_balance`.
   If fiber-pay is unavailable, fall back to `GET http://localhost:8081/fiber/channels`.
9. **Fiber wallet balance:** `fiber-pay wallet balance --json` → extract `balance_ckb`.
   This is the Fiber L2 balance, separate from CKB L1.

Format as a rich summary:

```
Agent Status: <lock_args>
──────────────────────────────────────
  Balance (L1):   <balance_ckb> CKB
  Spending limit: <spending_limit_ckb> CKB/tx
  Daily limit:    <daily_limit_ckb> CKB/day

  Reputation:
    Completed: <jobs_completed>
    Abandoned: <jobs_abandoned>
    Score:     <jobs_completed / (jobs_completed + jobs_abandoned) * 100>%

  Badges earned:  <badge_count>
  Capabilities:   <cap_count> (<comma-separated short hashes>)
  Active jobs:    <active_count>

  Fiber Network:
    Node:     <version> (<node_id short>)
    Peers:    <peers_count>
    Channels: <channel_count> (<total_local_balance> CKB)
    Wallet:   <fiber_wallet_balance> CKB (L2)
```

If any endpoint returns an error (404, 502, etc.), show "N/A" for that section and continue.

### get_capabilities

Fetch the agent's capability NFTs:
```
GET http://localhost:8081/agents/<lock_args>/capabilities
```

Return the list of `capability_hash` values and their outpoints.

## Result Format

Always return JSON:
```json
{
  "worker": "chain-scanner",
  "action": "<scan_jobs | get_balance | get_tx_status | get_agent | get_reputation | get_full_status | get_capabilities>",
  "status": "success | error",
  "data": { ... },
  "error": null
}
```

For `scan_jobs`, include a `jobs` array summarizing each cell:
```json
{
  "out_point": "0x<tx_hash>:0",
  "status": "Open",
  "reward_ckb": 5.0,
  "capability_hash": "0x...",
  "ttl_block_height": "12345678"
}
```
