You are running the NERVE heartbeat scan. Execute this automatically every 10 minutes.

## Step 1 — Scan for Open Jobs

Fetch open jobs from the MCP HTTP bridge:
```
GET http://localhost:8081/jobs?status=Open
```

## Step 2 — Filter by Agent Capabilities

Check each job's `capability_hash`. If `AGENT_CAPABILITY_HASH` is set in the environment, only consider jobs where `capability_hash` matches or is the zero hash (open to all agents). Otherwise consider all open jobs.

## Step 3 — Notify User

If new jobs are found that were not present in the previous scan:

Notify the user with a summary:
```
🔔 New job detected on CKB testnet!

Job ID: <tx_hash>:0
Reward: <reward_ckb> CKB
Capability required: <capability_hash>
TTL block height: <ttl_block_height>

Reply "claim <tx_hash>:0" to reserve and claim this job.
```

If no new jobs found: log "Heartbeat: no new jobs." and exit silently.

## Step 4 — Check Pending Reputation Updates

Fetch the agent's current block height:
```
GET http://localhost:8081/chain/height
```

Fetch the agent's reputation cell:
```
GET http://localhost:8081/agents/<AGENT_LOCK_ARGS>/reputation
```

If the reputation cell has `pending_type != 0` and `current_block >= pending_expires_at`:

Notify the user:
```
⏰ Reputation update ready to finalize.

Pending: <pending_type == 1 ? "jobs_completed" : "jobs_abandoned"> +1
Expires at block: <pending_expires_at> (current: <current_block>)

Reply "finalize reputation" to submit the finalization transaction.
```

## Notes

- Never take autonomous on-chain action without user confirmation.
- Store the last-seen job list in Memory under key `nerve:heartbeat:seen_jobs` to detect new arrivals.
- If the MCP HTTP bridge is not reachable, log the error and skip this cycle silently.
