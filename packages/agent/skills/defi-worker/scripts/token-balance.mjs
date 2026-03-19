#!/usr/bin/env node
// Token balance helper: query xUDT token balances on CKB testnet.
//
// Usage:
//   node token-balance.mjs --address <ckb_address> [--token <type_args>]
//   node token-balance.mjs --help
//
// Environment:
//   CKB_RPC_URL  CKB node RPC (default: https://testnet.ckb.dev/rpc)

import { parseArgs } from 'node:util';

const XUDT_CODE_HASH = '0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feebd97f6abf75bf7b5a0';

const { values } = parseArgs({
	options: {
		address: { type: 'string' },
		token: { type: 'string' },
		help: { type: 'boolean', default: false },
	},
});

if (values.help) {
	console.log(`Token balance helper: query xUDT balances on CKB testnet.

Usage:
  node token-balance.mjs --address <ckb_address> [--token <type_args>]

Options:
  --address  CKB address to check
  --token    xUDT type_args to filter (optional; omit to list all xUDT tokens)
  --help     Show this help message

Environment:
  CKB_RPC_URL  CKB node RPC endpoint`);
	process.exit(0);
}

if (!values.address) {
	console.error(JSON.stringify({ error: '--address is required. Use --help for usage.' }));
	process.exit(1);
}

const CKB_RPC = process.env.CKB_RPC_URL || 'https://testnet.ckb.dev/rpc';

async function rpcCall(method, params) {
	const resp = await fetch(CKB_RPC, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
	});
	const json = await resp.json();
	if (json.error) throw new Error(`RPC error: ${json.error.message}`);
	return json.result;
}

async function main() {
	// Derive lock script from address using ckb address decode.
	// For standard secp256k1 addresses, parse the lock hash from the address format.
	// We'll use the indexer's get_cells to search by lock script + type script.

	// Build a type script filter for xUDT cells.
	const typeScript = {
		code_hash: XUDT_CODE_HASH,
		hash_type: 'type',
		args: values.token || '0x',
	};

	// Use the indexer to search for cells with this type script owned by the address.
	// We need the lock script for the address; derive it from the address format.
	const lockScript = await addressToLockScript(values.address);

	const searchKey = {
		script: lockScript,
		script_type: 'lock',
		filter: { script: typeScript },
	};

	const cells = await rpcCall('get_cells', [searchKey, 'asc', '0x64']);

	// Aggregate balances by type_args.
	const balances = {};
	for (const cell of cells.objects || []) {
		const typeArgs = cell.output.type?.args || 'unknown';
		const data = cell.output_data || '0x';
		// xUDT amount is the first 16 bytes (u128 LE) of cell data.
		const hex = data.replace('0x', '').slice(0, 32);
		if (hex.length < 32) continue;
		const bytes = Buffer.from(hex, 'hex');
		const amount = bytes.readBigUInt64LE(0) + (bytes.readBigUInt64LE(8) << 64n);
		balances[typeArgs] = (balances[typeArgs] || 0n) + amount;
	}

	const result = {
		address: values.address,
		tokens: Object.entries(balances).map(([typeArgs, amount]) => ({
			type_args: typeArgs,
			balance: amount.toString(),
		})),
	};

	console.log(JSON.stringify(result, null, 2));
}

async function addressToLockScript(address) {
	// CKB testnet addresses (ckt1...) use bech32m encoding.
	// Standard secp256k1-blake160 lock:
	const SECP_CODE_HASH = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';

	if (address.startsWith('ckt1qz') || address.startsWith('ck1qz')) {
		// Short format: extract lock_args from the address payload.
		// For simplicity, fall back to asking the node if available.
	}

	// Use a simple heuristic: if the address is a standard short-form address,
	// we can extract the 20-byte lock_args from the bech32 payload.
	// For robustness, use the parse-address helper if available.
	try {
		const { bech32m } = await import('bech32');
		const decoded = bech32m.decode(address, 120);
		const payload = Buffer.from(bech32m.fromWords(decoded.words));
		// CKB full format: 0x00 (format) + code_hash (32) + hash_type (1) + args (20)
		if (payload.length >= 54 && payload[0] === 0x00) {
			const codeHash = '0x' + payload.subarray(1, 33).toString('hex');
			const hashType = payload[33] === 0x01 ? 'type' : 'data';
			const args = '0x' + payload.subarray(34).toString('hex');
			return { code_hash: codeHash, hash_type: hashType, args };
		}
		// Short format: 0x01 + code_hash_index (1) + args (20)
		if (payload[0] === 0x01) {
			const args = '0x' + payload.subarray(2).toString('hex');
			return { code_hash: SECP_CODE_HASH, hash_type: 'type', args };
		}
	} catch {
		// bech32 not available; fall through.
	}

	// Fallback: assume the address contains lock_args directly (0x-prefixed hex).
	if (address.startsWith('0x') && address.length === 42) {
		return { code_hash: SECP_CODE_HASH, hash_type: 'type', args: address };
	}

	throw new Error(`Cannot parse CKB address: ${address}. Provide lock_args (0x-prefixed 20-byte hex) directly.`);
}

main().catch((err) => {
	console.error(JSON.stringify({ error: err.message || String(err) }));
	process.exit(1);
});
