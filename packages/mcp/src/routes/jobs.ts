import { Router } from 'express';
import { getCellsByScript, getLiveCell, getTipBlockNumber, Script, LiveCell } from '../ckb.js';

const router = Router();

const CORE_URL = process.env.CORE_URL ?? 'http://localhost:8080';
const JOB_TYPE_CODE_HASH = process.env.JOB_CELL_TYPE_CODE_HASH ?? '';
const CAP_NFT_TYPE_CODE_HASH = process.env.CAP_NFT_TYPE_CODE_HASH ?? '';
const ZERO_CAPABILITY_HASH = '0x' + '0'.repeat(64);

const JOB_STATUS = ['Open', 'Reserved', 'Claimed', 'Completed', 'Expired'] as const;
type JobStatus = (typeof JOB_STATUS)[number];

interface ParsedJob {
	out_point: { tx_hash: string; index: string };
	status: JobStatus;
	poster_lock_args: string;
	worker_lock_args: string | null;
	reward_ckb: number;
	ttl_block_height: string;
	capability_hash: string;
	capacity_shannons: string;
}

function parseJobCell(cell: LiveCell): ParsedJob | null {
	const hexData = cell.output_data;
	if (!hexData || hexData === '0x' || hexData.length < 2 + 180) return null; // 90 bytes = 180 hex chars

	const raw = Buffer.from(hexData.replace('0x', ''), 'hex');
	if (raw.length < 90) return null;
	if (raw[0] !== 0) return null; // version check

	const statusByte = raw[1];
	if (statusByte > 4) return null;

	const posterLockArgs = '0x' + raw.subarray(2, 22).toString('hex');
	const workerBytes = raw.subarray(22, 42);
	const workerLockArgs = workerBytes.every((b) => b === 0)
		? null
		: '0x' + workerBytes.toString('hex');
	const rewardShannons = raw.readBigUInt64LE(42);
	const ttlBlockHeight = raw.readBigUInt64LE(50);
	const capabilityHash = '0x' + raw.subarray(58, 90).toString('hex');

	return {
		out_point: cell.out_point,
		status: JOB_STATUS[statusByte],
		poster_lock_args: posterLockArgs,
		worker_lock_args: workerLockArgs,
		reward_ckb: Number(rewardShannons) / 1e8,
		ttl_block_height: ttlBlockHeight.toString(),
		capability_hash: capabilityHash,
		capacity_shannons: BigInt(cell.output.capacity).toString(),
	};
}

// GET /jobs?status=Open&capability_hash=0x...
router.get('/', async (req, res) => {
	if (!JOB_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'JOB_CELL_TYPE_CODE_HASH not configured' });
		return;
	}

	const { status, capability_hash } = req.query as Record<string, string | undefined>;

	const script: Script = {
		code_hash: JOB_TYPE_CODE_HASH,
		hash_type: 'data1',
		args: '0x',
	};

	try {
		const result = await getCellsByScript(script, 'type', 200);
		let jobs = result.objects
			.map(parseJobCell)
			.filter((j): j is ParsedJob => j !== null);

		if (status && JOB_STATUS.includes(status as JobStatus)) {
			jobs = jobs.filter((j) => j.status === status);
		}
		if (capability_hash) {
			jobs = jobs.filter((j) => j.capability_hash === capability_hash);
		}

		res.json({ jobs, count: jobs.length });
	} catch (e) {
		console.error('jobs route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /jobs/match/:lock_args — Find open jobs matching an agent's capability NFTs.
// Returns jobs that are either open-to-all (zero capability_hash) or match a
// capability NFT held by the agent. Also filters out expired jobs.
router.get('/match/:lock_args', async (req, res) => {
	if (!JOB_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'JOB_CELL_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	try {
		// Fetch agent's capability NFTs.
		const agentCapabilities = new Set<string>();
		if (CAP_NFT_TYPE_CODE_HASH) {
			const capScript: Script = {
				code_hash: CAP_NFT_TYPE_CODE_HASH,
				hash_type: 'data1',
				args: '0x',
			};
			const capResult = await getCellsByScript(capScript, 'type', 200);
			for (const c of capResult.objects) {
				const raw = Buffer.from((c.output_data ?? '0x').replace('0x', ''), 'hex');
				if (raw.length < 54) continue;
				const agentArgs = '0x' + raw.subarray(2, 22).toString('hex');
				if (agentArgs.toLowerCase() === lock_args.toLowerCase()) {
					agentCapabilities.add('0x' + raw.subarray(22, 54).toString('hex'));
				}
			}
		}

		// Fetch current block height for TTL filtering.
		const currentBlock = await getTipBlockNumber();

		// Fetch all open jobs.
		const jobScript: Script = {
			code_hash: JOB_TYPE_CODE_HASH,
			hash_type: 'data1',
			args: '0x',
		};
		const jobResult = await getCellsByScript(jobScript, 'type', 200);
		const allJobs = jobResult.objects
			.map(parseJobCell)
			.filter((j): j is ParsedJob => j !== null);

		// Filter: open, not expired, and capability match.
		const matched = allJobs.filter((j) => {
			if (j.status !== 'Open') return false;
			if (BigInt(j.ttl_block_height) - currentBlock < 50n) return false;
			if (j.capability_hash === ZERO_CAPABILITY_HASH) return true;
			return agentCapabilities.has(j.capability_hash);
		});

		// Sort by reward descending.
		matched.sort((a, b) => b.reward_ckb - a.reward_ckb);

		res.json({
			lock_args,
			agent_capabilities: [...agentCapabilities],
			jobs: matched,
			count: matched.length,
		});
	} catch (e) {
		console.error('jobs match route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /jobs — post a new job (proxies to nerve-core TX builder).
// Body: { reward_ckb: number, ttl_blocks: number, capability_hash: string }
router.post('/', async (req, res) => {
	const { reward_ckb, ttl_blocks, capability_hash } = req.body as {
		reward_ckb?: number;
		ttl_blocks?: number;
		capability_hash?: string;
	};

	if (reward_ckb === undefined || typeof reward_ckb !== 'number' || reward_ckb <= 0) {
		res.status(400).json({ error: 'reward_ckb must be a positive number' });
		return;
	}
	if (ttl_blocks === undefined || typeof ttl_blocks !== 'number' || ttl_blocks <= 0) {
		res.status(400).json({ error: 'ttl_blocks must be a positive number' });
		return;
	}
	if (!capability_hash || typeof capability_hash !== 'string' || !/^0x[0-9a-fA-F]{64}$/.test(capability_hash)) {
		res.status(400).json({ error: 'capability_hash must be a 0x-prefixed 32-byte hex string' });
		return;
	}

	try {
		const response = await fetch(`${CORE_URL}/tx/build-and-broadcast`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ intent: 'post_job', reward_ckb, ttl_blocks, capability_hash }),
		});
		const data = await response.json();
		res.status(response.status).json(data);
	} catch (e) {
		res.status(502).json({ error: 'failed to reach nerve-core' });
	}
});

// GET /jobs/:tx_hash/:index — get a specific job cell by outpoint.
router.get('/:tx_hash/:index', async (req, res) => {
	const { tx_hash, index } = req.params;
	const indexNum = parseInt(index, 10);
	if (isNaN(indexNum)) {
		res.status(400).json({ error: 'index must be a number' });
		return;
	}

	try {
		const cell = await getLiveCell({ tx_hash, index: `0x${indexNum.toString(16)}` });
		if (!cell) {
			res.status(404).json({ error: 'cell not found' });
			return;
		}
		const job = parseJobCell(cell);
		if (!job) {
			res.status(422).json({ error: 'cell is not a valid job cell' });
			return;
		}
		res.json(job);
	} catch (e) {
		console.error('jobs route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

export default router;
