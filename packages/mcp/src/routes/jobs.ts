import { Router } from 'express';
import { getCellsByScript, getLiveCell, getTipBlockNumber, Script, LiveCell } from '../ckb.js';

const router = Router();

const CORE_URL = process.env.CORE_URL ?? 'http://localhost:8080';
const JOB_TYPE_CODE_HASH = process.env.JOB_CELL_TYPE_CODE_HASH ?? '';
const CAP_NFT_TYPE_CODE_HASH = process.env.CAP_NFT_TYPE_CODE_HASH ?? '';
const REP_TYPE_CODE_HASH = process.env.REPUTATION_TYPE_CODE_HASH ?? '';
const ZERO_CAPABILITY_HASH = '0x' + '0'.repeat(64);

const JOB_STATUS = ['Open', 'Reserved', 'Claimed', 'Completed', 'Expired'] as const;
type JobStatus = (typeof JOB_STATUS)[number];

interface JobPaymentMetadata {
	mode: 'fiber';
	lock_args?: string;
	node_id?: string;
	rpc_url?: string;
	description?: string;
}

interface ParsedJob {
	out_point: { tx_hash: string; index: string };
	status: JobStatus;
	poster_lock_args: string;
	worker_lock_args: string | null;
	reward_ckb: number;
	ttl_block_height: string;
	capability_hash: string;
	description_hash: string | null;
	description: string | null;
	payment?: JobPaymentMetadata | null;
	capacity_shannons: string;
}

function splitPaymentMetadata(description: string | null): { description: string | null, payment: JobPaymentMetadata | null } {
	if (!description) return { description, payment: null };
	const marker = '\n\nNERVE_PAYMENT:';
	const idx = description.indexOf(marker);
	if (idx === -1) return { description, payment: null };
	const base = description.slice(0, idx) || null;
	const raw = description.slice(idx + marker.length).trim();
	try {
		return { description: base, payment: JSON.parse(raw) as JobPaymentMetadata };
	} catch {
		return { description, payment: null };
	}
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

	const descriptionHash = raw.length >= 122
		? '0x' + raw.subarray(90, 122).toString('hex')
		: null;
	const rawDescription = raw.length > 122
		? raw.subarray(122).toString('utf-8')
		: null;
	const { description, payment } = splitPaymentMetadata(rawDescription);

	return {
		out_point: cell.out_point,
		status: JOB_STATUS[statusByte],
		poster_lock_args: posterLockArgs,
		worker_lock_args: workerLockArgs,
		reward_ckb: Number(rewardShannons) / 1e8,
		ttl_block_height: ttlBlockHeight.toString(),
		capability_hash: capabilityHash,
		description_hash: descriptionHash,
		description,
		payment,
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

// GET /jobs/match/:lock_args: Find open jobs matching an agent's capability NFTs.
// Returns jobs that are either open-to-all (zero capability_hash) or match a
// capability NFT held by the agent. Also filters out expired jobs.
router.get('/match/:lock_args', async (req, res) => {
	if (!JOB_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'JOB_CELL_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	try {
		// Parallel-fetch capabilities, block height, jobs, and reputation cells.
		const capPromise = CAP_NFT_TYPE_CODE_HASH
			? getCellsByScript({ code_hash: CAP_NFT_TYPE_CODE_HASH, hash_type: 'data1', args: '0x' }, 'type', 200)
			: Promise.resolve({ objects: [] as LiveCell[] });
		const repPromise = REP_TYPE_CODE_HASH
			? getCellsByScript({ code_hash: REP_TYPE_CODE_HASH, hash_type: 'data1', args: '0x' }, 'type', 200)
			: Promise.resolve({ objects: [] as LiveCell[] });
		const jobPromise = getCellsByScript(
			{ code_hash: JOB_TYPE_CODE_HASH, hash_type: 'data1', args: '0x' }, 'type', 200,
		);
		const [capResult, currentBlock, jobResult, repResult] = await Promise.all([
			capPromise, getTipBlockNumber(), jobPromise, repPromise,
		]);

		// Build agent capability set.
		const agentCapabilities = new Set<string>();
		for (const c of capResult.objects) {
			const raw = Buffer.from((c.output_data ?? '0x').replace('0x', ''), 'hex');
			if (raw.length < 54) continue;
			const agentArgs = '0x' + raw.subarray(2, 22).toString('hex');
			if (agentArgs.toLowerCase() === lock_args.toLowerCase()) {
				agentCapabilities.add('0x' + raw.subarray(22, 54).toString('hex'));
			}
		}

		// Build poster reputation lookup: lock_args → { completed, abandoned }.
		const posterRep = new Map<string, { completed: number; abandoned: number }>();
		for (const c of repResult.objects) {
			const raw = Buffer.from((c.output_data ?? '0x').replace('0x', ''), 'hex');
			if (raw.length < 46) continue;
			const agentArgs = '0x' + raw.subarray(26, 46).toString('hex');
			posterRep.set(agentArgs.toLowerCase(), {
				completed: Number(raw.readBigUInt64LE(2)),
				abandoned: Number(raw.readBigUInt64LE(10)),
			});
		}

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

		// reward_value (0-40):  normalized reward relative to the best offer.
		// ttl_urgency  (0-30):  prefer 50-200 blocks remaining, penalize too-soon or stale.
		// poster_rep   (0-30):  poster's reputation ratio, weighted by volume.
		const maxReward = matched.reduce((m, j) => Math.max(m, j.reward_ckb), 0) || 1;

		const scored = matched.map((j) => {
			const blocksLeft = Number(BigInt(j.ttl_block_height) - currentBlock);

			// Reward: linear 0-40 relative to best offer.
			const rewardScore = Math.round((j.reward_ckb / maxReward) * 40);

			// TTL urgency: peak at 100-200 blocks, taper outside.
			let urgencyScore: number;
			if (blocksLeft >= 100 && blocksLeft <= 200) {
				urgencyScore = 30; // sweet spot
			} else if (blocksLeft >= 50 && blocksLeft < 100) {
				urgencyScore = Math.round(((blocksLeft - 50) / 50) * 30);
			} else if (blocksLeft > 200 && blocksLeft <= 500) {
				urgencyScore = Math.round(((500 - blocksLeft) / 300) * 25) + 5;
			} else {
				urgencyScore = 5; // very distant jobs get minimum score
			}

			// Poster reputation.
			let posterScore = 15; // default for unknown posters
			const rep = posterRep.get(j.poster_lock_args.toLowerCase());
			if (rep) {
				const total = rep.completed + rep.abandoned;
				if (total > 0) {
					const ratio = rep.completed / total;
					const volume = Math.min(total / 5, 1);
					posterScore = Math.round(ratio * 30 * volume);
				}
			}

			const score = rewardScore + urgencyScore + posterScore;

			return { ...j, score, score_breakdown: { reward: rewardScore, urgency: urgencyScore, poster_reputation: posterScore } };
		});

		// Sort by composite score descending.
		scored.sort((a, b) => b.score - a.score);

		res.json({
			lock_args,
			agent_capabilities: [...agentCapabilities],
			jobs: scored,
			count: scored.length,
		});
	} catch (e) {
		console.error('jobs match route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /jobs: post a new job (proxies to nerve-core TX builder).
// Body: { reward_ckb, ttl_blocks, capability_hash, description?, payment? }
router.post('/', async (req, res) => {
	const { reward_ckb, ttl_blocks, capability_hash, description, payment } = req.body as {
		reward_ckb?: number;
		ttl_blocks?: number;
		capability_hash?: string;
		description?: string;
		payment?: JobPaymentMetadata;
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
	if (description !== undefined && typeof description !== 'string') {
		res.status(400).json({ error: 'description must be a string' });
		return;
	}
	if (payment !== undefined) {
		if (payment.mode !== 'fiber') {
			res.status(400).json({ error: 'payment.mode must be fiber' });
			return;
		}
	}

	try {
		const payload: Record<string, unknown> = { intent: 'post_job', reward_ckb, ttl_blocks, capability_hash };
		const fullDescription = payment
			? `${description ?? ''}\n\nNERVE_PAYMENT:${JSON.stringify(payment)}`
			: description;
		if (fullDescription !== undefined) payload.description = fullDescription;
		const response = await fetch(`${CORE_URL}/tx/build-and-broadcast`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(payload),
		});
		const data = await response.json();
		res.status(response.status).json(data);
	} catch (e) {
		res.status(502).json({ error: 'failed to reach nerve-core' });
	}
});

// GET /jobs/:tx_hash/:index: get a specific job cell by outpoint.
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

// GET /jobs/stream: SSE endpoint for real-time job state changes.
// Polls the CKB indexer every 10s and emits events when jobs are created, updated, or consumed.
router.get('/stream', (req, res) => {
	if (!JOB_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'JOB_CELL_TYPE_CODE_HASH not configured' });
		return;
	}

	res.setHeader('Content-Type', 'text/event-stream');
	res.setHeader('Cache-Control', 'no-cache');
	res.setHeader('Connection', 'keep-alive');
	res.flushHeaders();

	const prevJobs = new Map<string, ParsedJob>();
	let stopped = false;

	// Guard against writes to a destroyed stream.
	res.on('error', () => { stopped = true; });

	const emit = (event: string, data: unknown) => {
		if (!stopped) res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
	};

	const poll = async () => {
		if (stopped) return;
		try {
			const script: Script = {
				code_hash: JOB_TYPE_CODE_HASH,
				hash_type: 'data1',
				args: '0x',
			};
			const result = await getCellsByScript(script, 'type', 200);
			const currentJobs = new Map<string, ParsedJob>();

			for (const cell of result.objects) {
				const job = parseJobCell(cell);
				if (!job) continue;
				const key = `${job.out_point.tx_hash}:${job.out_point.index}`;
				currentJobs.set(key, job);

				const prev = prevJobs.get(key);
				if (!prev) {
					emit(`job:${job.status.toLowerCase()}`, job);
				} else if (prev.status !== job.status) {
					emit(`job:${job.status.toLowerCase()}`, job);
				}
			}

			// Detect consumed (expired/removed) cells.
			for (const [key, prev] of prevJobs) {
				if (!currentJobs.has(key)) {
					emit('job:expired', prev);
				}
			}

			prevJobs.clear();
			for (const [key, job] of currentJobs) {
				prevJobs.set(key, job);
			}

			// Heartbeat keeps the connection alive through reverse proxies.
			if (!stopped) res.write(': heartbeat\n\n');
		} catch (e) {
			console.error('SSE poll error:', e);
		}
	};

	// Poll-then-schedule pattern prevents concurrent polls.
	const scheduleNext = () => {
		if (stopped) return;
		setTimeout(async () => {
			await poll();
			scheduleNext();
		}, 10_000);
	};
	poll();
	scheduleNext();

	req.on('close', () => { stopped = true; });
});

export default router;
