// TypeScript CKB transaction builder — molecule serialization, signing support,
// cell collection, cell data encoders, and unsigned intent builders.
//
// Ported from packages/core/src/tx_builder/{molecule,signing,identity,job,transfer}.rs.
// Uses the blake2b npm package with CKB "ckb-default-hash" personalization.

import blake2b from 'blake2b';
import { getCellsByScript, getLiveCell, getTipBlockNumber, Script, LiveCell } from './ckb.js';

export const SECP256K1_CODE_HASH =
	'0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
export const SECP256K1_DEP_TX_HASH =
	'0xf8de3bb47d055cdf460d93a2a6e1b05f7432f9777c8c474abf4eec1d4aee5d37';
const SECP256K1_HASH_TYPE = 'type';

const MIN_CELL_CAPACITY = 61n * 100_000_000n;
const IDENTITY_CELL_CAPACITY = 232n * 100_000_000n;
const REP_CELL_CAPACITY = 172n * 100_000_000n;
const JOB_CELL_OVERHEAD = 184n * 100_000_000n;
const ESTIMATED_FEE = 2_000_000n;

function ckbHash(data: Buffer): Buffer {
	const personal = Buffer.alloc(16);
	personal.write('ckb-default-hash');
	const h = blake2b(32, undefined, undefined, personal);
	h.update(data);
	return Buffer.from(h.digest());
}

function hexToBuffer(hex: string): Buffer {
	return Buffer.from(hex.replace(/^0x/i, ''), 'hex');
}

function bufferToHex(buf: Buffer): string {
	return '0x' + buf.toString('hex');
}

function hexToU32(hex: string): number {
	return parseInt(hex.replace(/^0x/i, ''), 16);
}

function hexToU64(hex: string): bigint {
	return BigInt(hex);
}

function u32LE(n: number): Buffer {
	const buf = Buffer.alloc(4);
	buf.writeUInt32LE(n, 0);
	return buf;
}

function u64LE(n: bigint): Buffer {
	const buf = Buffer.alloc(8);
	buf.writeBigUInt64LE(n, 0);
	return buf;
}

export function serializeTable(fields: Buffer[]): Buffer {
	const headerSize = 4 + fields.length * 4;
	const dataSize = fields.reduce((sum, f) => sum + f.length, 0);
	const totalSize = headerSize + dataSize;

	const buf = Buffer.alloc(totalSize);
	let pos = 0;
	buf.writeUInt32LE(totalSize, pos); pos += 4;

	let offset = headerSize;
	for (const field of fields) {
		buf.writeUInt32LE(offset, pos); pos += 4;
		offset += field.length;
	}

	for (const field of fields) {
		field.copy(buf, pos);
		pos += field.length;
	}

	return buf;
}

export function serializeFixVec(items: Buffer[]): Buffer {
	const dataSize = items.reduce((sum, i) => sum + i.length, 0);
	const buf = Buffer.alloc(4 + dataSize);
	buf.writeUInt32LE(items.length, 0);
	let pos = 4;
	for (const item of items) {
		item.copy(buf, pos);
		pos += item.length;
	}
	return buf;
}

export function serializeDynVec(items: Buffer[]): Buffer {
	if (items.length === 0) {
		return u32LE(4);
	}

	const headerSize = 4 + items.length * 4;
	const dataSize = items.reduce((sum, i) => sum + i.length, 0);
	const totalSize = headerSize + dataSize;

	const buf = Buffer.alloc(totalSize);
	let pos = 0;
	buf.writeUInt32LE(totalSize, pos); pos += 4;

	let offset = headerSize;
	for (const item of items) {
		buf.writeUInt32LE(offset, pos); pos += 4;
		offset += item.length;
	}

	for (const item of items) {
		item.copy(buf, pos);
		pos += item.length;
	}

	return buf;
}

export function serializeBytes(data: Buffer): Buffer {
	const buf = Buffer.alloc(4 + data.length);
	buf.writeUInt32LE(data.length, 0);
	data.copy(buf, 4);
	return buf;
}

interface TxScript {
	code_hash: string;
	hash_type: string;
	args: string;
}

interface TxOutPoint {
	tx_hash: string;
	index: string;
}

interface TxCellDep {
	out_point: TxOutPoint;
	dep_type: string;
}

interface TxCellInput {
	since: string;
	previous_output: TxOutPoint;
}

interface TxCellOutput {
	capacity: string;
	lock: TxScript;
	type: TxScript | null;
}

export interface UnsignedTx {
	version: string;
	cell_deps: TxCellDep[];
	header_deps: string[];
	inputs: TxCellInput[];
	outputs: TxCellOutput[];
	outputs_data: string[];
	witnesses: string[];
}

function hashTypeByte(ht: string): number {
	switch (ht) {
		case 'data': return 0;
		case 'type': return 1;
		case 'data1': return 2;
		case 'data2': return 4;
		default: throw new Error(`invalid hash_type: ${ht}`);
	}
}

function depTypeByte(dt: string): number {
	switch (dt) {
		case 'code': return 0;
		case 'dep_group': return 1;
		default: throw new Error(`invalid dep_type: ${dt}`);
	}
}

export function serializeScript(script: TxScript): Buffer {
	const codeHash = hexToBuffer(script.code_hash);
	if (codeHash.length !== 32) throw new Error(`script code_hash must be 32 bytes, got ${codeHash.length}`);
	const htByte = Buffer.from([hashTypeByte(script.hash_type)]);
	const args = serializeBytes(hexToBuffer(script.args));
	return serializeTable([codeHash, htByte, args]);
}

export function serializeOutPoint(op: TxOutPoint): Buffer {
	const txHash = hexToBuffer(op.tx_hash);
	if (txHash.length !== 32) throw new Error(`out_point tx_hash must be 32 bytes`);
	const index = u32LE(hexToU32(op.index));
	return Buffer.concat([txHash, index]);
}

export function serializeCellDep(dep: TxCellDep): Buffer {
	const op = serializeOutPoint(dep.out_point);
	const dt = Buffer.from([depTypeByte(dep.dep_type)]);
	return Buffer.concat([op, dt]);
}

export function serializeCellInput(input: TxCellInput): Buffer {
	const since = u64LE(hexToU64(input.since));
	const prevOutput = serializeOutPoint(input.previous_output);
	return Buffer.concat([since, prevOutput]);
}

export function serializeCellOutput(output: TxCellOutput): Buffer {
	const capacity = u64LE(hexToU64(output.capacity));
	const lock = serializeScript(output.lock);
	const typeScript = output.type ? serializeScript(output.type) : Buffer.alloc(0);
	return serializeTable([capacity, lock, typeScript]);
}

export function serializeRawTransaction(tx: UnsignedTx): Buffer {
	const version = u32LE(hexToU32(tx.version));
	const cellDeps = serializeFixVec(tx.cell_deps.map(serializeCellDep));
	const headerDeps = serializeFixVec(tx.header_deps.map(h => hexToBuffer(h)));
	const inputs = serializeFixVec(tx.inputs.map(serializeCellInput));
	const outputs = serializeDynVec(tx.outputs.map(serializeCellOutput));
	const outputsData = serializeDynVec(
		tx.outputs_data.map(d => serializeBytes(hexToBuffer(d))),
	);
	return serializeTable([version, cellDeps, headerDeps, inputs, outputs, outputsData]);
}

export function computeRawTxHash(tx: UnsignedTx): string {
	const raw = serializeRawTransaction(tx);
	return bufferToHex(ckbHash(raw));
}

export function placeholderWitness(): Buffer {
	// WitnessArgs Table: lock=Some([0;65]), input_type=None, output_type=None.
	const buf = Buffer.alloc(85);
	let pos = 0;
	buf.writeUInt32LE(85, pos); pos += 4;   // total_size
	buf.writeUInt32LE(16, pos); pos += 4;   // offset_lock
	buf.writeUInt32LE(85, pos); pos += 4;   // offset_input_type
	buf.writeUInt32LE(85, pos); pos += 4;   // offset_output_type
	buf.writeUInt32LE(65, pos);             // lock_len
	// Bytes [20..85] are already zero (placeholder).
	return buf;
}

export function computeSigningMessage(
	txHash: string,
	witnessPlaceholder: Buffer,
	additionalWitnesses: Buffer[],
): string {
	const txHashBuf = hexToBuffer(txHash);
	if (txHashBuf.length !== 32) throw new Error('tx_hash must be 32 bytes');

	const personal = Buffer.alloc(16);
	personal.write('ckb-default-hash');
	const h = blake2b(32, undefined, undefined, personal);

	h.update(txHashBuf);

	// First witness (placeholder with lock field zeroed).
	h.update(u64LE(BigInt(witnessPlaceholder.length)));
	h.update(witnessPlaceholder);

	// Additional witnesses in the lock group.
	for (const w of additionalWitnesses) {
		h.update(u64LE(BigInt(w.length)));
		h.update(w);
	}

	return bufferToHex(Buffer.from(h.digest()));
}

export function injectSignature(tx: UnsignedTx, signatureHex: string): UnsignedTx {
	const sig = hexToBuffer(signatureHex);
	if (sig.length !== 65) throw new Error(`signature must be 65 bytes, got ${sig.length}`);

	const witness = placeholderWitness();
	sig.copy(witness, 20); // Write signature at bytes [20..85].
	const clone = { ...tx, witnesses: [...tx.witnesses] };
	clone.witnesses[0] = bufferToHex(witness);
	return clone;
}

function secp256k1Lock(lockArgs: string): Script {
	return {
		code_hash: SECP256K1_CODE_HASH,
		hash_type: SECP256K1_HASH_TYPE,
		args: lockArgs,
	};
}

export async function collectPlainCells(
	lockArgs: string,
	needed: bigint,
): Promise<{ inputs: TxCellInput[]; capacity: bigint; firstTxHash: string; firstIndex: number }> {
	const script = secp256k1Lock(lockArgs);
	const result = await getCellsByScript(script, 'lock', 200);

	const inputs: TxCellInput[] = [];
	let capacity = 0n;
	let firstTxHash = '';
	let firstIndex = 0;

	for (const cell of result.objects) {
		// Skip typed cells to avoid consuming protocol cells.
		if (cell.output.type !== null) continue;

		const cap = hexToU64(cell.output.capacity);
		if (firstTxHash === '') {
			firstTxHash = cell.out_point.tx_hash;
			firstIndex = hexToU32(cell.out_point.index);
		}
		inputs.push({
			since: '0x0',
			previous_output: cell.out_point,
		});
		capacity += cap;
		if (capacity >= needed + MIN_CELL_CAPACITY) break;
	}

	if (capacity < needed + MIN_CELL_CAPACITY) {
		throw new Error(
			`insufficient funds: need ${Number(needed + MIN_CELL_CAPACITY) / 1e8} CKB, ` +
			`have ${Number(capacity) / 1e8} CKB`,
		);
	}

	if (firstTxHash === '') {
		throw new Error('no plain cells available');
	}

	return { inputs, capacity, firstTxHash, firstIndex };
}

export function calculateTypeId(
	firstInputTxHash: string,
	firstInputIndex: number,
	since: bigint,
	outputIndex: bigint,
): string {
	const txHash = hexToBuffer(firstInputTxHash);
	if (txHash.length !== 32) throw new Error('type_id: tx_hash must be 32 bytes');

	// CellInput molecule: since(8) + tx_hash(32) + index(4).
	const cellInput = Buffer.alloc(44);
	cellInput.writeBigUInt64LE(since, 0);
	txHash.copy(cellInput, 8);
	cellInput.writeUInt32LE(firstInputIndex, 40);

	const personal = Buffer.alloc(16);
	personal.write('ckb-default-hash');
	const h = blake2b(32, undefined, undefined, personal);
	h.update(cellInput);
	h.update(u64LE(outputIndex));
	return bufferToHex(Buffer.from(h.digest()));
}

export function encodeIdentityData(
	pubkey: Buffer,
	spendingLimitShannons: bigint,
	dailyLimitShannons: bigint,
): Buffer {
	if (pubkey.length !== 33) throw new Error('pubkey must be 33 bytes (compressed)');
	const buf = Buffer.alloc(50);
	buf[0] = 0; // version
	pubkey.copy(buf, 1);
	buf.writeBigUInt64LE(spendingLimitShannons, 34);
	buf.writeBigUInt64LE(dailyLimitShannons, 42);
	return buf;
}

export function encodeJobData(
	posterLockArgs: Buffer,
	workerLockArgs: Buffer,
	rewardShannons: bigint,
	ttlBlockHeight: bigint,
	capabilityHash: Buffer,
): Buffer {
	if (posterLockArgs.length !== 20) throw new Error('poster_lock_args must be 20 bytes');
	if (workerLockArgs.length !== 20) throw new Error('worker_lock_args must be 20 bytes');
	if (capabilityHash.length !== 32) throw new Error('capability_hash must be 32 bytes');
	const buf = Buffer.alloc(90);
	buf[0] = 0; // version
	buf[1] = 0; // status: Open
	posterLockArgs.copy(buf, 2);
	workerLockArgs.copy(buf, 22);
	buf.writeBigUInt64LE(rewardShannons, 42);
	buf.writeBigUInt64LE(ttlBlockHeight, 50);
	capabilityHash.copy(buf, 58);
	return buf;
}

export function encodeRepData(
	pendingType: number,
	completed: bigint,
	abandoned: bigint,
	expiresAt: bigint,
	agentLockArgs: Buffer,
): Buffer {
	if (agentLockArgs.length !== 20) throw new Error('agent_lock_args must be 20 bytes');
	const buf = Buffer.alloc(46);
	buf[0] = 0; // version
	buf[1] = pendingType;
	buf.writeBigUInt64LE(completed, 2);
	buf.writeBigUInt64LE(abandoned, 10);
	buf.writeBigUInt64LE(expiresAt, 18);
	agentLockArgs.copy(buf, 26);
	return buf;
}

function placeholderWitnesses(count: number): string[] {
	const ph = bufferToHex(placeholderWitness());
	return Array.from({ length: count }, (_, i) => (i === 0 ? ph : '0x'));
}

function requireEnv(name: string): string {
	const val = process.env[name];
	if (!val) throw new Error(`${name} not set — deploy contracts first.`);
	return val;
}

function formatCap(shannons: bigint): string {
	return '0x' + shannons.toString(16);
}

interface TemplateResult {
	tx: UnsignedTx;
	tx_hash: string;
	signing_message: string;
}

function buildTemplate(tx: UnsignedTx): TemplateResult {
	const txHash = computeRawTxHash(tx);
	const placeholder = placeholderWitness();
	const additional = tx.witnesses.slice(1).map(w => hexToBuffer(w));
	const signingMessage = computeSigningMessage(txHash, placeholder, additional);
	return { tx, tx_hash: txHash, signing_message: signingMessage };
}

async function fetchJobCell(
	jobTxHash: string,
	jobIndex: number,
): Promise<{ capacity: bigint; data: Buffer }> {
	const cell = await getLiveCell({ tx_hash: jobTxHash, index: '0x' + jobIndex.toString(16) }, true);
	if (!cell) throw new Error(`job cell ${jobTxHash}:${jobIndex} not found or not live`);

	const capacity = hexToU64(cell.output.capacity);
	const data = hexToBuffer(cell.output_data);

	if (data.length < 90) throw new Error('job cell data too short');
	return { capacity, data };
}

export async function buildSpawnAgent(
	lockArgs: string,
	params: { pubkey: string; spending_limit_ckb: number; daily_limit_ckb: number },
): Promise<TemplateResult> {
	const typeCodeHash = requireEnv('AGENT_IDENTITY_TYPE_CODE_HASH');
	const depTxHash = requireEnv('AGENT_IDENTITY_DEP_TX_HASH');

	const pubkey = hexToBuffer(params.pubkey);
	const spendingLimit = BigInt(Math.round(params.spending_limit_ckb * 1e8));
	const dailyLimit = BigInt(Math.round(params.daily_limit_ckb * 1e8));

	const needed = IDENTITY_CELL_CAPACITY + ESTIMATED_FEE;
	const { inputs, capacity, firstTxHash, firstIndex } = await collectPlainCells(lockArgs, needed);

	const typeIdArgs = calculateTypeId(firstTxHash, firstIndex, 0n, 0n);
	const changeCapacity = capacity - IDENTITY_CELL_CAPACITY - ESTIMATED_FEE;
	const identityData = encodeIdentityData(pubkey, spendingLimit, dailyLimit);
	const lock = secp256k1Lock(lockArgs);

	const tx: UnsignedTx = {
		version: '0x0',
		cell_deps: [
			{ out_point: { tx_hash: SECP256K1_DEP_TX_HASH, index: '0x0' }, dep_type: 'dep_group' },
			{ out_point: { tx_hash: depTxHash, index: '0x0' }, dep_type: 'code' },
		],
		header_deps: [],
		inputs,
		outputs: [
			{
				capacity: formatCap(IDENTITY_CELL_CAPACITY),
				lock,
				type: { code_hash: typeCodeHash, hash_type: 'data1', args: typeIdArgs },
			},
			{ capacity: formatCap(changeCapacity), lock, type: null },
		],
		outputs_data: [bufferToHex(identityData), '0x'],
		witnesses: placeholderWitnesses(inputs.length),
	};

	return buildTemplate(tx);
}

export async function buildCreateReputation(
	lockArgs: string,
): Promise<TemplateResult> {
	const typeCodeHash = requireEnv('REPUTATION_TYPE_CODE_HASH');
	const depTxHash = requireEnv('REPUTATION_DEP_TX_HASH');

	const agentLockArgs = hexToBuffer(lockArgs);
	const repData = encodeRepData(0, 0n, 0n, 0n, agentLockArgs);

	const needed = REP_CELL_CAPACITY + ESTIMATED_FEE;
	const { inputs, capacity, firstTxHash, firstIndex } = await collectPlainCells(lockArgs, needed);

	const typeIdArgs = calculateTypeId(firstTxHash, firstIndex, 0n, 0n);
	const changeCapacity = capacity - REP_CELL_CAPACITY - ESTIMATED_FEE;
	const lock = secp256k1Lock(lockArgs);

	const tx: UnsignedTx = {
		version: '0x0',
		cell_deps: [
			{ out_point: { tx_hash: SECP256K1_DEP_TX_HASH, index: '0x0' }, dep_type: 'dep_group' },
			{ out_point: { tx_hash: depTxHash, index: '0x0' }, dep_type: 'code' },
		],
		header_deps: [],
		inputs,
		outputs: [
			{
				capacity: formatCap(REP_CELL_CAPACITY),
				lock,
				type: { code_hash: typeCodeHash, hash_type: 'data1', args: typeIdArgs },
			},
			{ capacity: formatCap(changeCapacity), lock, type: null },
		],
		outputs_data: [bufferToHex(repData), '0x'],
		witnesses: placeholderWitnesses(inputs.length),
	};

	return buildTemplate(tx);
}

export async function buildPostJob(
	lockArgs: string,
	params: { reward_ckb: number; ttl_blocks: number; capability_hash?: string },
): Promise<TemplateResult> {
	const typeCodeHash = requireEnv('JOB_CELL_TYPE_CODE_HASH');
	const depTxHash = requireEnv('JOB_CELL_DEP_TX_HASH');

	const rewardShannons = BigInt(Math.round(params.reward_ckb * 1e8));
	const tip = await getTipBlockNumber();
	const ttlBlockHeight = tip + BigInt(params.ttl_blocks);

	const posterLockArgs = hexToBuffer(lockArgs);
	const capHash = params.capability_hash
		? hexToBuffer(params.capability_hash)
		: Buffer.alloc(32);

	const jobData = encodeJobData(posterLockArgs, Buffer.alloc(20), rewardShannons, ttlBlockHeight, capHash);
	const jobCellCapacity = JOB_CELL_OVERHEAD + rewardShannons;

	const needed = jobCellCapacity + ESTIMATED_FEE;
	const { inputs, capacity } = await collectPlainCells(lockArgs, needed);

	const changeCapacity = capacity - jobCellCapacity - ESTIMATED_FEE;
	const lock = secp256k1Lock(lockArgs);

	const tx: UnsignedTx = {
		version: '0x0',
		cell_deps: [
			{ out_point: { tx_hash: SECP256K1_DEP_TX_HASH, index: '0x0' }, dep_type: 'dep_group' },
			{ out_point: { tx_hash: depTxHash, index: '0x0' }, dep_type: 'code' },
		],
		header_deps: [],
		inputs,
		outputs: [
			{
				capacity: formatCap(jobCellCapacity),
				lock,
				type: { code_hash: typeCodeHash, hash_type: 'data1', args: '0x' },
			},
			{ capacity: formatCap(changeCapacity), lock, type: null },
		],
		outputs_data: [bufferToHex(jobData), '0x'],
		witnesses: placeholderWitnesses(inputs.length),
	};

	return buildTemplate(tx);
}

export async function buildReserveJob(
	lockArgs: string,
	params: { job_tx_hash: string; job_index: number },
): Promise<TemplateResult> {
	const typeCodeHash = requireEnv('JOB_CELL_TYPE_CODE_HASH');
	const depTxHash = requireEnv('JOB_CELL_DEP_TX_HASH');

	const { capacity: jobCapacity, data: jobData } = await fetchJobCell(params.job_tx_hash, params.job_index);
	if (jobData[1] !== 0) throw new Error(`job status is ${jobData[1]}, expected Open(0)`);

	// Set status to Reserved, write worker lock_args.
	jobData[1] = 1;
	const workerBytes = hexToBuffer(lockArgs);
	if (workerBytes.length !== 20) throw new Error('lock_args must be 20 bytes');
	workerBytes.copy(jobData, 22);

	const { inputs: feeInputs, capacity: feeCapacity } = await collectPlainCells(lockArgs, ESTIMATED_FEE);
	const changeCapacity = feeCapacity - ESTIMATED_FEE;

	const jobInput: TxCellInput = {
		since: '0x0',
		previous_output: { tx_hash: params.job_tx_hash, index: '0x' + params.job_index.toString(16) },
	};
	const allInputs = [jobInput, ...feeInputs];
	const lock = secp256k1Lock(lockArgs);

	const tx: UnsignedTx = {
		version: '0x0',
		cell_deps: [
			{ out_point: { tx_hash: SECP256K1_DEP_TX_HASH, index: '0x0' }, dep_type: 'dep_group' },
			{ out_point: { tx_hash: depTxHash, index: '0x0' }, dep_type: 'code' },
		],
		header_deps: [],
		inputs: allInputs,
		outputs: [
			{
				capacity: formatCap(jobCapacity),
				lock,
				type: { code_hash: typeCodeHash, hash_type: 'data1', args: '0x' },
			},
			{ capacity: formatCap(changeCapacity), lock, type: null },
		],
		outputs_data: [bufferToHex(jobData), '0x'],
		witnesses: placeholderWitnesses(allInputs.length),
	};

	return buildTemplate(tx);
}

export async function buildClaimJob(
	lockArgs: string,
	params: { job_tx_hash: string; job_index: number },
): Promise<TemplateResult> {
	const typeCodeHash = requireEnv('JOB_CELL_TYPE_CODE_HASH');
	const depTxHash = requireEnv('JOB_CELL_DEP_TX_HASH');

	const { capacity: jobCapacity, data: jobData } = await fetchJobCell(params.job_tx_hash, params.job_index);
	if (jobData[1] !== 1) throw new Error(`job status is ${jobData[1]}, expected Reserved(1)`);

	// Set status to Claimed.
	jobData[1] = 2;

	const { inputs: feeInputs, capacity: feeCapacity } = await collectPlainCells(lockArgs, ESTIMATED_FEE);
	const changeCapacity = feeCapacity - ESTIMATED_FEE;

	const jobInput: TxCellInput = {
		since: '0x0',
		previous_output: { tx_hash: params.job_tx_hash, index: '0x' + params.job_index.toString(16) },
	};
	const allInputs = [jobInput, ...feeInputs];
	const lock = secp256k1Lock(lockArgs);

	const tx: UnsignedTx = {
		version: '0x0',
		cell_deps: [
			{ out_point: { tx_hash: SECP256K1_DEP_TX_HASH, index: '0x0' }, dep_type: 'dep_group' },
			{ out_point: { tx_hash: depTxHash, index: '0x0' }, dep_type: 'code' },
		],
		header_deps: [],
		inputs: allInputs,
		outputs: [
			{
				capacity: formatCap(jobCapacity),
				lock,
				type: { code_hash: typeCodeHash, hash_type: 'data1', args: '0x' },
			},
			{ capacity: formatCap(changeCapacity), lock, type: null },
		],
		outputs_data: [bufferToHex(jobData), '0x'],
		witnesses: placeholderWitnesses(allInputs.length),
	};

	return buildTemplate(tx);
}

export async function buildCompleteJob(
	lockArgs: string,
	params: { job_tx_hash: string; job_index: number; worker_lock_args: string; result_hash?: string },
): Promise<TemplateResult> {
	const depTxHash = requireEnv('JOB_CELL_DEP_TX_HASH');

	const { capacity: jobCapacity, data: jobData } = await fetchJobCell(params.job_tx_hash, params.job_index);
	if (jobData[1] !== 2) throw new Error(`job status is ${jobData[1]}, expected Claimed(2)`);

	const rewardShannons = jobData.readBigUInt64LE(42);
	const posterRefund = jobCapacity - rewardShannons;

	if (rewardShannons < MIN_CELL_CAPACITY) {
		throw new Error(`reward ${Number(rewardShannons) / 1e8} CKB is below minimum cell capacity (61 CKB)`);
	}

	const workerLock = secp256k1Lock(params.worker_lock_args);
	const posterLock = secp256k1Lock(lockArgs);
	const posterRefundAfterFee = posterRefund - ESTIMATED_FEE;

	const inputs: TxCellInput[] = [{
		since: '0x0',
		previous_output: { tx_hash: params.job_tx_hash, index: '0x' + params.job_index.toString(16) },
	}];

	const outputs: TxCellOutput[] = [
		{ capacity: formatCap(rewardShannons), lock: workerLock, type: null },
		{ capacity: formatCap(posterRefundAfterFee), lock: posterLock, type: null },
	];
	const outputsData = ['0x', '0x'];

	const tx: UnsignedTx = {
		version: '0x0',
		cell_deps: [
			{ out_point: { tx_hash: SECP256K1_DEP_TX_HASH, index: '0x0' }, dep_type: 'dep_group' },
			{ out_point: { tx_hash: depTxHash, index: '0x0' }, dep_type: 'code' },
		],
		header_deps: [],
		inputs,
		outputs,
		outputs_data: outputsData,
		witnesses: placeholderWitnesses(inputs.length),
	};

	return buildTemplate(tx);
}

export async function buildTransfer(
	lockArgs: string,
	params: { to_lock_args: string; amount_ckb: number },
): Promise<TemplateResult> {
	const amountShannons = BigInt(Math.round(params.amount_ckb * 1e8));
	if (amountShannons < MIN_CELL_CAPACITY) {
		throw new Error(`amount ${params.amount_ckb} CKB is below minimum cell capacity (61 CKB)`);
	}

	const needed = amountShannons + ESTIMATED_FEE;
	const { inputs, capacity } = await collectPlainCells(lockArgs, needed);

	const changeCapacity = capacity - amountShannons - ESTIMATED_FEE;
	const senderLock = secp256k1Lock(lockArgs);
	const recipientLock = secp256k1Lock(params.to_lock_args);

	const tx: UnsignedTx = {
		version: '0x0',
		cell_deps: [
			{ out_point: { tx_hash: SECP256K1_DEP_TX_HASH, index: '0x0' }, dep_type: 'dep_group' },
		],
		header_deps: [],
		inputs,
		outputs: [
			{ capacity: formatCap(amountShannons), lock: recipientLock, type: null },
			{ capacity: formatCap(changeCapacity), lock: senderLock, type: null },
		],
		outputs_data: ['0x', '0x'],
		witnesses: placeholderWitnesses(inputs.length),
	};

	return buildTemplate(tx);
}
