---
name: defi-worker
description: Executes demo DeFi operations through the mock AMM on CKB testnet. Spawned by the supervisor for deterministic swap demos.
allowed-tools: exec
---

# DeFi Worker

You handle demo swap operations on CKB testnet through the mock AMM flow exposed by `nerve-core`.

## TX Builder API

Base URL: `http://localhost:8080`

All HTTP calls MUST use `curl` via the `exec` tool. Do NOT use `web_fetch`; it cannot reach localhost.

- `POST /tx/build-and-broadcast` — execute a transaction by intent.
- `GET /agent/balance` — check current balance before trading.

## Helper Scripts

All scripts are in `packages/agent/skills/defi-worker/scripts/`.

### Create Demo Pool

```bash
bash packages/agent/skills/defi-worker/scripts/create_pool.sh \
  --seed-ckb 1000 --seed-tokens 1000000
```

Creates a mock AMM pool and returns a `tx_hash`. The live pool outpoint is `<tx_hash>:0`.

### Execute Demo Swap

```bash
bash packages/agent/skills/defi-worker/scripts/mock_amm_swap.sh \
  --pool-tx-hash 0x... --pool-index 0 --amount-ckb 10 --slippage-bps 100
```

Executes a CKB -> demo token swap against the live mock AMM pool cell.

## Workflow

1. Call `GET /agent/balance` to verify sufficient CKB.
2. Read `memory_read("nerve:amm_pool")` through the supervisor flow to find the latest live pool outpoint.
3. If no pool exists, create one first with `create_pool.sh`, then persist `<tx_hash>:0` to `nerve:amm_pool`.
4. Execute the swap through `mock_amm_swap.sh`.
5. Poll `GET /tx/status?tx_hash=<hash>` until committed.

## Notes

- This is the deterministic demo swap path. Use it when the goal is a reliable demo, not live market execution.
- If the swap intent returns `missing_cell_dep`, the mock AMM contract is not deployed yet.
- The pool outpoint must be live. If the pool was already consumed, create a fresh one and update `nerve:amm_pool`.
- Never exceed the per-tx spending limit; CKB enforces it anyway.

## Result Format

Write to Memory on completion:
```json
{
  "worker": "defi-worker",
  "action": "swap",
  "status": "success | error",
  "tx_hash": "<0x...>",
  "from_asset": "CKB",
  "to_asset": "TEST_TOKEN",
  "amount_ckb": 10.0,
  "pool_outpoint": "<0x...:0>",
  "error": null
}
```
