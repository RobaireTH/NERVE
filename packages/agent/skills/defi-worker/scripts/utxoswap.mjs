#!/usr/bin/env node
// UTXOSwap helper: execute CKB/xUDT swaps on CKB testnet via the UTXOSwap SDK.
//
// Usage:
//   node utxoswap.mjs --from CKB --to <type_args_hash> --amount <ckb_amount> [--slippage <bps>]
//   node utxoswap.mjs --help
//
// Environment:
//   CKB_RPC_URL       CKB node RPC (default: https://testnet.ckb.dev/rpc)
//   UTXOSWAP_API_KEY  UTXOSwap API key (get from utxoswap.xyz)

import { parseArgs } from 'node:util';

const { values } = parseArgs({
	options: {
		from: { type: 'string', default: 'CKB' },
		to: { type: 'string' },
		amount: { type: 'string' },
		slippage: { type: 'string', default: '100' },
		help: { type: 'boolean', default: false },
	},
});

if (values.help) {
	console.log(`UTXOSwap helper: swap CKB for xUDT tokens on CKB testnet.

Usage:
  node utxoswap.mjs --from CKB --to <type_args> --amount <ckb> [--slippage <bps>]

Options:
  --from       Source asset (default: CKB)
  --to         Target token type_args hash (0x-prefixed)
  --amount     Amount of source asset to swap (in CKB)
  --slippage   Slippage tolerance in basis points (default: 100 = 1%)
  --help       Show this help message

Environment:
  CKB_RPC_URL       CKB node RPC endpoint
  UTXOSWAP_API_KEY  UTXOSwap API key

Example:
  node utxoswap.mjs --from CKB --to 0xabcd...1234 --amount 100 --slippage 50`);
	process.exit(0);
}

if (!values.to || !values.amount) {
	console.error(JSON.stringify({ error: '--to and --amount are required. Use --help for usage.' }));
	process.exit(1);
}

const CKB_RPC = process.env.CKB_RPC_URL || 'https://testnet.ckb.dev/rpc';
const API_KEY = process.env.UTXOSWAP_API_KEY || '';

async function main() {
	let SDK;
	try {
		SDK = await import('@utxoswap/swap-sdk-js');
	} catch {
		console.error(JSON.stringify({
			error: 'Failed to import @utxoswap/swap-sdk-js. Run: npm install @utxoswap/swap-sdk-js',
		}));
		process.exit(1);
	}

	const { Collector, Client, Pool, Token } = SDK;

	const collector = new Collector({ ckbNodeUrl: CKB_RPC });
	const client = new Client({
		apiKey: API_KEY,
		isMainnet: false,
	});

	const fromToken = Token.CKB;
	const toToken = new Token({ typeArgs: values.to });
	const amountIn = BigInt(Math.round(parseFloat(values.amount) * 1e8));
	const slippageBps = parseInt(values.slippage, 10);

	// Find best pool for this pair.
	const pools = await client.getPools({ tokenA: fromToken, tokenB: toToken });
	if (!pools || pools.length === 0) {
		console.error(JSON.stringify({
			error: `No UTXOSwap pool found for CKB <-> ${values.to}`,
			hint: 'Check that a pool exists on UTXOSwap testnet for this token pair.',
		}));
		process.exit(1);
	}

	// Pick pool with deepest liquidity.
	const pool = pools.reduce((best, p) =>
		(p.liquidity > best.liquidity ? p : best), pools[0]);

	// Calculate expected output.
	const quote = pool.getQuote({
		tokenIn: fromToken,
		amountIn,
		slippageBps,
	});

	const result = {
		pool_id: pool.id,
		input_token: values.from,
		output_token: values.to,
		input_amount: amountIn.toString(),
		expected_output: quote.amountOut.toString(),
		minimum_output: quote.minimumAmountOut.toString(),
		price_impact_bps: quote.priceImpactBps,
		slippage_bps: slippageBps,
		status: 'quote',
		note: 'Full swap execution requires a signing callback. Use the supervisor agent to execute swaps.',
	};

	console.log(JSON.stringify(result, null, 2));
}

main().catch((err) => {
	console.error(JSON.stringify({
		error: err.message || String(err),
		stack: err.stack,
	}));
	process.exit(1);
});
