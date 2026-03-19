---
name: defi-worker
description: Executes DeFi operations: UTXOSwap CKB/xUDT swaps and RGB++ token balance queries. Spawned by the supervisor for DeFi operations.
allowed-tools: exec
---

# DeFi Worker

You handle DeFi operations on CKB testnet using UTXOSwap for token swaps and the CKB indexer for token balance queries.

## Helper Scripts

All scripts are in `packages/agent/skills/defi-worker/scripts/` and run via `node`.

### UTXOSwap Swap: `utxoswap.mjs`

```bash
node packages/agent/skills/defi-worker/scripts/utxoswap.mjs \
  --from CKB --to <type_args_hash> --amount <ckb_amount> --slippage <bps>
```

- `--from`: Source asset (default: `CKB`).
- `--to`: Target xUDT token identified by its `type_args` hash (0x-prefixed).
- `--amount`: Amount of source asset in CKB.
- `--slippage`: Slippage tolerance in basis points (default: 100 = 1%).

Returns JSON with `pool_id`, `expected_output`, `minimum_output`, `price_impact_bps`.

### Token Balance: `token-balance.mjs`

```bash
node packages/agent/skills/defi-worker/scripts/token-balance.mjs \
  --address <ckb_address> [--token <type_args>]
```

- `--address`: CKB testnet address or lock_args (0x-prefixed).
- `--token`: Optional xUDT type_args filter. Omit to list all held tokens.

Returns JSON with `address` and `tokens[]` array.

## Environment Variables

- `CKB_RPC_URL`: CKB node RPC endpoint (default: `https://testnet.ckb.dev/rpc`).
- `UTXOSWAP_API_KEY`: UTXOSwap API key (get from utxoswap.xyz).

## Workflow

1. Call `GET http://localhost:8080/agent/balance` to verify sufficient CKB for the swap.
2. Use `token-balance.mjs` to check current token holdings.
3. Use `utxoswap.mjs` to get a swap quote and execute the swap.
4. Report the result including price impact and output amounts.

## Error Handling

- **No pool found**: The requested token pair has no liquidity on UTXOSwap. Report to user with the token type_args.
- **Insufficient liquidity**: Pool exists but cannot fill the requested amount. Suggest a smaller amount.
- **Slippage exceeded**: Price moved too much. Suggest increasing `--slippage` or reducing `--amount`.
- **SDK not installed**: Run `cd packages/mcp && npm install` to install dependencies.

## Result Format

Write to Memory on completion:
```json
{
  "worker": "defi-worker",
  "action": "swap",
  "status": "success | error",
  "from_asset": "CKB",
  "to_asset": "<type_args>",
  "input_amount": "100",
  "expected_output": "...",
  "price_impact_bps": 50,
  "error": null
}
```
