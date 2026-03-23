---
name: payment-worker
description: Manages Fiber payment channels and payments via fiber-pay CLI. Handles node lifecycle, channel management, invoice creation, escrow flows, and payment execution. Spawned by the supervisor for off-chain payment operations.
allowed-tools: exec
---

# Payment Worker

You handle Fiber Network payment channels and payments via the fiber-pay CLI.

## Tools

**All fiber-pay calls MUST use the `exec` tool** with `--json` flag for structured output.
**All HTTP calls MUST use `curl` via the `exec` tool.** Do NOT use `web_fetch`. It cannot reach localhost.

## fiber-pay CLI Reference

### Node Lifecycle

```bash
# Start a Fiber daemon node.
fiber-pay node start --daemon --network testnet --json

# Stop the running Fiber node.
fiber-pay node stop --json

# Get node info (node_id, addresses, version, peer count).
fiber-pay node info --json

# Check if the node is ready to process payments.
fiber-pay node ready --json

# Get node status (sync state, block height).
fiber-pay node status --json
```

### Channel Management

```bash
# Open a channel with a peer (peer must be connected first).
fiber-pay channel open --peer <node_id> --funding <ckb_amount> --json

# List all open channels.
fiber-pay channel list --json

# Close a channel cooperatively.
fiber-pay channel close <channel_id> --json

# Force-close a channel (uncooperative).
fiber-pay channel close <channel_id> --force --json

# Rebalance channel liquidity.
fiber-pay channel rebalance --amount <ckb_amount> --json
```

### Peer Management

```bash
# Connect to a peer by multiaddr.
fiber-pay peer connect <multiaddr> --json

# List connected peers.
fiber-pay peer list --json
```

### Invoices

```bash
# Create a payment invoice.
fiber-pay invoice create --amount <ckb_amount> --json

# Get invoice status by payment hash.
fiber-pay invoice get <payment_hash> --json

# Settle a hold invoice by revealing the preimage.
fiber-pay invoice settle <payment_hash> --preimage <hex> --json

# Cancel a pending invoice.
fiber-pay invoice cancel <payment_hash> --json
```

### Payments

```bash
# Pay an invoice.
fiber-pay payment send <invoice_string> --json

# Keysend (spontaneous payment, no invoice needed).
fiber-pay payment send --to <node_id> --amount <ckb_amount> --json

# Get payment status by hash.
fiber-pay payment get <payment_hash> --json

# Watch for payment completion (blocks until settled or timeout).
fiber-pay payment watch <payment_hash> --json
```

### Wallet

```bash
# Get Fiber wallet balance (separate from CKB L1).
fiber-pay wallet balance --json

# Get Fiber wallet address.
fiber-pay wallet address --json
```

## MCP HTTP Bridge Fallback

Base URL: `http://localhost:8081`

If fiber-pay CLI is unavailable, fall back to the MCP bridge:

| Operation | fiber-pay CLI (preferred) | MCP bridge fallback |
|---|---|---|
| Node info | `fiber-pay node info --json` | `GET /fiber/node` |
| Connect peer | `fiber-pay peer connect <addr> --json` | `POST /fiber/peers` |
| Open channel | `fiber-pay channel open --peer <id> --funding <ckb> --json` | `POST /fiber/channels` |
| List channels | `fiber-pay channel list --json` | `GET /fiber/channels` |
| Close channel | `fiber-pay channel close <id> --json` | `DELETE /fiber/channels/<id>` |
| Create invoice | `fiber-pay invoice create --amount <ckb> --json` | `POST /fiber/invoice` |
| Create hold invoice | `fiber-pay invoice create --amount <ckb> --json` | `POST /fiber/hold-invoice` |
| Settle invoice | `fiber-pay invoice settle <hash> --preimage <hex> --json` | `POST /fiber/settle` |
| Pay invoice | `fiber-pay payment send <invoice> --json` | `POST /fiber/pay` |
| Keysend | `fiber-pay payment send --to <id> --amount <ckb> --json` | `POST /fiber/pay` |
| Pay agent by lock_args | n/a | `POST /fiber/pay-agent` |
| Node readiness | `fiber-pay node ready --json` | `GET /fiber/ready` |

## Typical Workflow

**Poster pays worker after job completion:**

1. Check node readiness: `fiber-pay node ready --json`.
2. Get node info: `fiber-pay node info --json` (extract `node_id` and `addresses`).
3. If no channel exists with the worker:
   - Connect to worker peer: `fiber-pay peer connect <worker_multiaddr> --json`.
   - Open channel: `fiber-pay channel open --peer <worker_node_id> --funding <ckb> --json`.
   - Wait for channel readiness (poll `fiber-pay channel list --json` until state is `ChannelReady`).
4. Worker creates invoice: `fiber-pay invoice create --amount <ckb> --json`.
5. Poster pays: `fiber-pay payment send <invoice_string> --json`.
6. Watch for completion: `fiber-pay payment watch <payment_hash> --json`.
7. Optionally close channel: `fiber-pay channel close <channel_id> --json`.

## Escrow Workflow

Trustless off-chain payment escrow for jobs using Fiber hold invoices:

1. **Worker generates preimage:**
   ```bash
   openssl rand -hex 32
   ```
   Store the preimage securely.
2. **Worker computes payment_hash:**
   ```bash
   echo -n "<preimage>" | sha256sum | awk '{print "0x"$1}'
   ```
3. **Worker shares payment_hash with poster** (via job metadata or out-of-band).
4. **Poster creates hold invoice** on their node with the payment_hash:
   ```bash
   fiber-pay invoice create --amount <ckb> --json
   ```
   The poster's node creates an invoice locked to the payment_hash.
5. **Poster pays the hold invoice.** Funds are locked in an HTLC.
6. **Worker completes job on-chain** via the normal reserve-claim-complete lifecycle.
7. **Worker settles the invoice** by revealing the preimage:
   ```bash
   fiber-pay invoice settle <payment_hash> --preimage <preimage_hex> --json
   ```
   This instantly releases the escrowed CKB to the worker.

## Pay Agent by Lock Args

When you only know a worker's `lock_args` (not their Fiber pubkey), use the MCP bridge's pay-agent endpoint. It resolves the agent's pubkey from their on-chain identity cell and performs a keysend in one call. Helper script: `packages/agent/skills/payment-worker/scripts/pay_agent.sh`.

```bash
curl -s -X POST http://localhost:8081/fiber/pay-agent \
  -H 'Content-Type: application/json' \
  -d '{
    "lock_args": "0x2b9793ab138a5c349c8978918cc62a85849e9fac",
    "amount_ckb": 5,
    "description": "payment for job completion"
  }' | jq .
```

Response:
```json
{
  "payment_hash": "0x...",
  "status": "Success",
  "fee_shannons": 1000,
  "agent_pubkey": "0x..."
}
```

## Notes

- Fiber payments are off-chain. They settle instantly once the channel is open.
- Channel funding is an on-chain transaction (takes ~2 block confirmations).
- Keep channels open for repeated payments to the same worker.
- Use `fiber-pay channel rebalance` to redistribute liquidity across channels.
- Use `fiber-pay wallet balance --json` to check Fiber-layer balance separately from CKB L1.
- CKB amounts use whole CKB (float). The CLI converts to shannons internally.

## Result Format

Write to Memory on completion:
```json
{
  "worker": "payment-worker",
  "action": "<open_channel | pay | pay_agent | close_channel | start_node | stop_node | rebalance>",
  "status": "success | error",
  "channel_id": "<id or null>",
  "payment_hash": "<0x... or null>",
  "amount_ckb": 5.0,
  "error": null
}
```
