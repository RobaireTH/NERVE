// CKB RPC + Indexer client for the MCP HTTP bridge.
// Uses native fetch (Node 20+). All amounts are in shannons (bigint).

const RPC_URL = process.env.CKB_RPC_URL ?? 'https://testnet.ckb.dev/rpc';
const INDEXER_URL = process.env.CKB_INDEXER_URL ?? 'https://testnet.ckb.dev/indexer';

let _rpcId = 1;

async function rpc<T>(url: string, method: string, params: unknown[]): Promise<T> {
	const res = await fetch(url, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ jsonrpc: '2.0', id: _rpcId++, method, params }),
	});
	const json = (await res.json()) as { result?: T; error?: { message: string } };
	if (json.error) throw new Error(`CKB RPC ${method}: ${json.error.message}`);
	if (json.result === undefined) throw new Error(`CKB RPC ${method}: empty result`);
	return json.result;
}

export interface Script {
	code_hash: string;
	hash_type: string;
	args: string;
}

export interface OutPoint {
	tx_hash: string;
	index: string;
}

export interface CellOutput {
	capacity: string;
	lock: Script;
	type: Script | null;
}

export interface LiveCell {
	output: CellOutput;
	output_data: string;
	out_point: OutPoint;
	block_number: string;
}

export interface GetCellsResult {
	objects: LiveCell[];
	last_cursor: string;
}

export function parseHexU64(hex: string): bigint {
	return BigInt(hex);
}

export async function getTipBlockNumber(): Promise<bigint> {
	const result = await rpc<string>(RPC_URL, 'get_tip_block_number', []);
	return parseHexU64(result);
}

export async function getCellsByScript(
	script: Script,
	scriptType: 'lock' | 'type',
	limit = 100,
): Promise<GetCellsResult> {
	return rpc<GetCellsResult>(INDEXER_URL, 'get_cells', [
		{ script, script_type: scriptType },
		'asc',
		`0x${limit.toString(16)}`,
	]);
}

export async function getLiveCell(outPoint: OutPoint, withData = true): Promise<LiveCell | null> {
	const result = await rpc<{ cell: LiveCell | null }>(RPC_URL, 'get_live_cell', [
		outPoint,
		withData,
	]);
	return result.cell;
}

export async function getTransaction(txHash: string): Promise<unknown> {
	return rpc<unknown>(RPC_URL, 'get_transaction', [txHash]);
}

// Compute total capacity (shannons) from a list of cells with a given lock.
export async function getBalanceByLock(lockArgs: string): Promise<bigint> {
	const script: Script = {
		code_hash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
		hash_type: 'type',
		args: lockArgs,
	};
	const result = await getCellsByScript(script, 'lock', 200);
	return result.objects.reduce((sum, c) => sum + parseHexU64(c.output.capacity), 0n);
}
