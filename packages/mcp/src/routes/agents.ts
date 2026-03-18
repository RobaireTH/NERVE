import { Router } from 'express';
import blake2b from 'blake2b';
import { getCellsByScript, getBalanceByLock, getTipBlockNumber, Script } from '../ckb.js';

const router = Router();

const AGENT_TYPE_CODE_HASH = process.env.AGENT_IDENTITY_TYPE_CODE_HASH ?? '';
const REP_TYPE_CODE_HASH = process.env.REPUTATION_TYPE_CODE_HASH ?? '';
const DOB_BADGE_CODE_HASH = process.env.DOB_BADGE_CODE_HASH ?? '';
const CAP_NFT_TYPE_CODE_HASH = process.env.CAP_NFT_TYPE_CODE_HASH ?? '';

// Identity cell data layout (88 bytes):
//   version(1) + pubkey(33) + spending_limit(8) + daily_limit(8)
//   + parent_lock_args(20) + revenue_share_bps(2)
//   + daily_spent(8) + last_reset_epoch(8)

export interface AgentInfo {
	lock_args: string;
	pubkey: string;
	spending_limit_ckb: number;
	daily_limit_ckb: number;
	parent_lock_args: string;
	revenue_share_bps: number;
	daily_spent_ckb: number;
	last_reset_epoch: number;
}

export function parseAgentCell(outputData: string, lockArgs: string): AgentInfo | null {
	if (!outputData || outputData === '0x') return null;
	const raw = Buffer.from(outputData.replace('0x', ''), 'hex');
	if (raw.length < 88) return null;
	if (raw[0] !== 0) return null;
	return {
		lock_args: lockArgs,
		pubkey: '0x' + raw.subarray(1, 34).toString('hex'),
		spending_limit_ckb: Number(raw.readBigUInt64LE(34)) / 1e8,
		daily_limit_ckb: Number(raw.readBigUInt64LE(42)) / 1e8,
		parent_lock_args: '0x' + raw.subarray(50, 70).toString('hex'),
		revenue_share_bps: raw.readUInt16LE(70),
		daily_spent_ckb: Number(raw.readBigUInt64LE(72)) / 1e8,
		last_reset_epoch: Number(raw.readBigUInt64LE(80)),
	};
}

// Reputation cell data layout (110 bytes):
//   version(1) + pending_type(1) + jobs_completed(8) + jobs_abandoned(8)
//   + pending_expires_at(8) + agent_lock_args(20) + proof_root(32)
//   + pending_settlement_hash(32)

interface ReputationInfo {
	agent_lock_args: string;
	jobs_completed: number;
	jobs_abandoned: number;
	pending_type: number;
	pending_expires_at: string;
	proof_root: string;
	pending_settlement_hash: string;
}

function parseReputationCell(outputData: string): ReputationInfo | null {
	if (!outputData || outputData === '0x') return null;
	const raw = Buffer.from(outputData.replace('0x', ''), 'hex');
	if (raw.length < 110) return null;
	if (raw[0] !== 0) return null;
	return {
		agent_lock_args: '0x' + raw.subarray(26, 46).toString('hex'),
		jobs_completed: Number(raw.readBigUInt64LE(2)),
		jobs_abandoned: Number(raw.readBigUInt64LE(10)),
		pending_type: raw[1],
		pending_expires_at: raw.readBigUInt64LE(18).toString(),
		proof_root: '0x' + raw.subarray(46, 78).toString('hex'),
		pending_settlement_hash: '0x' + raw.subarray(78, 110).toString('hex'),
	};
}

// Compute blake2b with CKB personalization ("ckb-default-hash").
function ckbBlake2b(data: Uint8Array): Buffer {
	const personal = Buffer.alloc(16);
	personal.write('ckb-default-hash');
	const h = blake2b(32, undefined, undefined, personal);
	h.update(data);
	return Buffer.from(h.digest());
}

// GET /agents/:lock_args — look up agent identity cell for a given lock_args.
router.get('/:lock_args', async (req, res) => {
	if (!AGENT_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'AGENT_IDENTITY_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	const script: Script = {
		code_hash: AGENT_TYPE_CODE_HASH,
		hash_type: 'data1',
		args: '0x',
	};

	try {
		const result = await getCellsByScript(script, 'type', 200);
		const match = result.objects.find(
			(c) => c.output.lock.args.toLowerCase() === lock_args.toLowerCase(),
		);
		if (!match) {
			res.status(404).json({ error: 'no agent identity cell found for this lock_args' });
			return;
		}
		const agent = parseAgentCell(match.output_data, lock_args);
		if (!agent) {
			res.status(422).json({ error: 'cell is not a valid agent identity cell' });
			return;
		}
		res.json({ agent, out_point: match.out_point });
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/reputation — look up reputation cell for an agent.
router.get('/:lock_args/reputation', async (req, res) => {
	if (!REP_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'REPUTATION_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	const script: Script = {
		code_hash: REP_TYPE_CODE_HASH,
		hash_type: 'data1',
		args: '0x',
	};

	try {
		const result = await getCellsByScript(script, 'type', 200);
		const match = result.objects.find((c) => {
			const rep = parseReputationCell(c.output_data);
			return rep && rep.agent_lock_args.toLowerCase() === lock_args.toLowerCase();
		});
		if (!match) {
			res.status(404).json({ error: 'no reputation cell found for this agent' });
			return;
		}
		const rep = parseReputationCell(match.output_data);
		res.json({ reputation: rep, out_point: match.out_point });
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/reputation/status — dispute window status for pending reputation proposals.
router.get('/:lock_args/reputation/status', async (req, res) => {
	if (!REP_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'REPUTATION_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	const script: Script = {
		code_hash: REP_TYPE_CODE_HASH,
		hash_type: 'data1',
		args: '0x',
	};

	try {
		const [result, tipBlock] = await Promise.all([
			getCellsByScript(script, 'type', 200),
			getTipBlockNumber(),
		]);

		const match = result.objects.find((c) => {
			const rep = parseReputationCell(c.output_data);
			return rep && rep.agent_lock_args.toLowerCase() === lock_args.toLowerCase();
		});
		if (!match) {
			res.status(404).json({ error: 'no reputation cell found for this agent' });
			return;
		}

		const rep = parseReputationCell(match.output_data);
		if (!rep) {
			res.status(422).json({ error: 'cell is not a valid reputation cell' });
			return;
		}

		const PENDING_LABELS = ['none', 'job_completed', 'job_abandoned'] as const;
		const expiresAt = BigInt(rep.pending_expires_at);
		const blocksRemaining = expiresAt > tipBlock ? Number(expiresAt - tipBlock) : 0;
		const canFinalize = rep.pending_type !== 0 && expiresAt <= tipBlock;

		res.json({
			pending: {
				type: rep.pending_type,
				label: PENDING_LABELS[rep.pending_type] ?? 'unknown',
				expires_at: rep.pending_expires_at,
				blocks_remaining: blocksRemaining,
				can_finalize: canFinalize,
			},
			jobs_completed: rep.jobs_completed,
			jobs_abandoned: rep.jobs_abandoned,
			current_block: tipBlock.toString(),
			out_point: match.out_point,
		});
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/spending — daily spending status from on-chain accumulator.
router.get('/:lock_args/spending', async (req, res) => {
	if (!AGENT_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'AGENT_IDENTITY_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;
	const script: Script = { code_hash: AGENT_TYPE_CODE_HASH, hash_type: 'data1', args: '0x' };

	try {
		const result = await getCellsByScript(script, 'type', 200);
		const match = result.objects.find(
			(c) => c.output.lock.args.toLowerCase() === lock_args.toLowerCase(),
		);
		if (!match) {
			res.status(404).json({ error: 'no agent identity cell found for this lock_args' });
			return;
		}
		const agent = parseAgentCell(match.output_data, lock_args);
		if (!agent) {
			res.status(422).json({ error: 'cell is not a valid agent identity cell' });
			return;
		}
		const dailyRemaining = Math.max(agent.daily_limit_ckb - agent.daily_spent_ckb, 0);
		const utilizationPct = agent.daily_limit_ckb > 0
			? Math.round((agent.daily_spent_ckb / agent.daily_limit_ckb) * 100)
			: 0;

		res.json({
			lock_args,
			daily_limit_ckb: agent.daily_limit_ckb,
			daily_spent_ckb: agent.daily_spent_ckb,
			daily_remaining_ckb: dailyRemaining,
			utilization_pct: utilizationPct,
			last_reset_epoch: agent.last_reset_epoch,
			out_point: match.out_point,
		});
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/badges — list PoP badges for an agent.
router.get('/:lock_args/badges', async (req, res) => {
	if (!DOB_BADGE_CODE_HASH) {
		res.status(503).json({ error: 'DOB_BADGE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	const script: Script = {
		code_hash: DOB_BADGE_CODE_HASH,
		hash_type: 'type',
		args: '0x',
	};

	try {
		const result = await getCellsByScript(script, 'type', 200);
		const badges = result.objects
			.filter((c) => c.output.lock.args.toLowerCase() === lock_args.toLowerCase())
			.map((c) => {
				const argsHex = c.output.type?.args ?? '0x';
				const argsRaw = Buffer.from(argsHex.replace('0x', ''), 'hex');
				const dataHex = c.output_data ?? '0x';
				const dataRaw = Buffer.from(dataHex.replace('0x', ''), 'hex');
				return {
					out_point: c.out_point,
					type_id: argsRaw.length >= 20 ? '0x' + argsRaw.subarray(0, 20).toString('hex') : null,
					event_id_hash: argsRaw.length >= 40 ? '0x' + argsRaw.subarray(20, 40).toString('hex') : null,
					recipient_hash: argsRaw.length >= 60 ? '0x' + argsRaw.subarray(40, 60).toString('hex') : null,
					content_hash: dataRaw.length >= 34 ? '0x' + dataRaw.subarray(2, 34).toString('hex') : null,
				};
			});
		res.json({ badges, count: badges.length });
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/capabilities — list capability NFTs for an agent.
router.get('/:lock_args/capabilities', async (req, res) => {
	if (!CAP_NFT_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'CAP_NFT_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	const script: Script = {
		code_hash: CAP_NFT_TYPE_CODE_HASH,
		hash_type: 'data1',
		args: '0x',
	};

	try {
		const result = await getCellsByScript(script, 'type', 200);
		// Capability data layout: [0] version, [1] proof_type, [2..22] agent_lock_args, [22..54] capability_hash.
		const capabilities = result.objects
			.filter((c) => {
				const dataHex = c.output_data ?? '0x';
				const raw = Buffer.from(dataHex.replace('0x', ''), 'hex');
				if (raw.length < 54) return false;
				const agentArgs = '0x' + raw.subarray(2, 22).toString('hex');
				return agentArgs.toLowerCase() === lock_args.toLowerCase();
			})
			.map((c) => {
				const raw = Buffer.from((c.output_data ?? '0x').replace('0x', ''), 'hex');
				const proofType = raw[1];
				const entry: Record<string, unknown> = {
					out_point: c.out_point,
					capability_hash: '0x' + raw.subarray(22, 54).toString('hex'),
					proof_type: proofType,
				};
				// proof_type=1: reputation-chain-backed with 64-byte proof data.
				if (proofType === 1 && raw.length >= 118) {
					entry.reputation_proof_root = '0x' + raw.subarray(54, 86).toString('hex');
					entry.settlement_hash = '0x' + raw.subarray(86, 118).toString('hex');
				}
				return entry;
			});
		res.json({ capabilities, count: capabilities.length });
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/reputation/verify — verify settlement hashes against on-chain proof_root.
router.get('/:lock_args/reputation/verify', async (req, res) => {
	if (!REP_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'REPUTATION_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;
	const hashesParam = req.query.settlement_hashes as string;
	if (!hashesParam) {
		res.status(400).json({ error: 'settlement_hashes query parameter required (comma-separated 0x hex)' });
		return;
	}

	const script: Script = {
		code_hash: REP_TYPE_CODE_HASH,
		hash_type: 'data1',
		args: '0x',
	};

	try {
		const result = await getCellsByScript(script, 'type', 200);
		const match = result.objects.find((c) => {
			const rep = parseReputationCell(c.output_data);
			return rep && rep.agent_lock_args.toLowerCase() === lock_args.toLowerCase();
		});
		if (!match) {
			res.status(404).json({ error: 'no reputation cell found for this agent' });
			return;
		}
		const rep = parseReputationCell(match.output_data);
		if (!rep) {
			res.status(422).json({ error: 'cell is not a valid reputation cell' });
			return;
		}

		// Parse settlement hashes.
		const hashes = hashesParam.split(',').map((h) => h.trim());
		const hashBuffers = hashes.map((h) => Buffer.from(h.replace('0x', ''), 'hex'));

		// Replay the hash chain from genesis (all zeros).
		let root = Buffer.alloc(32);
		for (const sh of hashBuffers) {
			const preimage = Buffer.concat([root, sh]);
			root = Buffer.from(ckbBlake2b(preimage)) as Buffer<ArrayBuffer>;
		}

		const computedRoot = '0x' + root.toString('hex');
		const onChainRoot = rep.proof_root;
		const verified = computedRoot === onChainRoot;

		res.json({
			verified,
			chain_length: hashes.length,
			computed_root: computedRoot,
			on_chain_root: onChainRoot,
		});
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/sub-agents — list sub-agents whose parent_lock_args matches this agent.
router.get('/:lock_args/sub-agents', async (req, res) => {
	if (!AGENT_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'AGENT_IDENTITY_TYPE_CODE_HASH not configured' });
		return;
	}

	const { lock_args } = req.params;

	const script: Script = {
		code_hash: AGENT_TYPE_CODE_HASH,
		hash_type: 'data1',
		args: '0x',
	};

	try {
		const result = await getCellsByScript(script, 'type', 200);
		const subAgents = result.objects
			.map((c) => {
				const agent = parseAgentCell(c.output_data, c.output.lock.args);
				if (!agent) return null;
				if (
					agent.parent_lock_args.toLowerCase() === '0x' + '00'.repeat(20) ||
					agent.parent_lock_args.toLowerCase() !== lock_args.toLowerCase()
				) {
					return null;
				}
				return {
					lock_args: agent.lock_args,
					parent_lock_args: agent.parent_lock_args,
					revenue_share_bps: agent.revenue_share_bps,
					spending_limit_ckb: agent.spending_limit_ckb,
					daily_limit_ckb: agent.daily_limit_ckb,
					out_point: c.out_point,
				};
			})
			.filter(Boolean);

		res.json({ sub_agents: subAgents, count: subAgents.length });
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /agents/:lock_args/trust — composite trust score synthesized from on-chain state.
// Parallel-fetches identity, reputation, capabilities, badges, and balance, then
// computes a normalized 0-100 score with a full breakdown.
router.get('/:lock_args/trust', async (req, res) => {
	const { lock_args } = req.params;

	try {
		// Build all script queries we might need.
		const fetches: Record<string, Promise<unknown>> = {};

		if (AGENT_TYPE_CODE_HASH) {
			const script: Script = { code_hash: AGENT_TYPE_CODE_HASH, hash_type: 'data1', args: '0x' };
			fetches.identity = getCellsByScript(script, 'type', 200);
		}
		if (REP_TYPE_CODE_HASH) {
			const script: Script = { code_hash: REP_TYPE_CODE_HASH, hash_type: 'data1', args: '0x' };
			fetches.reputation = getCellsByScript(script, 'type', 200);
		}
		if (CAP_NFT_TYPE_CODE_HASH) {
			const script: Script = { code_hash: CAP_NFT_TYPE_CODE_HASH, hash_type: 'data1', args: '0x' };
			fetches.capabilities = getCellsByScript(script, 'type', 200);
		}
		if (DOB_BADGE_CODE_HASH) {
			const script: Script = { code_hash: DOB_BADGE_CODE_HASH, hash_type: 'type', args: '0x' };
			fetches.badges = getCellsByScript(script, 'type', 200);
		}

		const results = await Promise.all(
			Object.entries(fetches).map(async ([key, promise]) => {
				try { return [key, await promise] as const; }
				catch { return [key, null] as const; }
			}),
		);
		const data: Record<string, { objects: Array<{ output: { lock: { args: string }; type?: { args: string } | null; capacity: string }; output_data: string; out_point: { tx_hash: string; index: string } }> } | null> = {};
		for (const [key, val] of results) data[key] = val as typeof data[string];

		// 1. Identity check (required).
		let identity: AgentInfo | null = null;
		if (data.identity) {
			const match = data.identity.objects.find(
				(c) => c.output.lock.args.toLowerCase() === lock_args.toLowerCase(),
			);
			if (match) identity = parseAgentCell(match.output_data, lock_args);
		}
		if (!identity) {
			res.status(404).json({ error: 'no agent identity cell found for this lock_args' });
			return;
		}

		// 2. Reputation.
		let jobsCompleted = 0;
		let jobsAbandoned = 0;
		let hasProofChain = false;
		if (data.reputation) {
			const match = data.reputation.objects.find((c) => {
				const rep = parseReputationCell(c.output_data);
				return rep && rep.agent_lock_args.toLowerCase() === lock_args.toLowerCase();
			});
			if (match) {
				const rep = parseReputationCell(match.output_data);
				if (rep) {
					jobsCompleted = rep.jobs_completed;
					jobsAbandoned = rep.jobs_abandoned;
					hasProofChain = rep.proof_root !== '0x' + '00'.repeat(32);
				}
			}
		}

		// 3. Capabilities.
		let capCount = 0;
		let chainBackedCaps = 0;
		if (data.capabilities) {
			for (const c of data.capabilities.objects) {
				const raw = Buffer.from((c.output_data ?? '0x').replace('0x', ''), 'hex');
				if (raw.length < 54) continue;
				const agentArgs = '0x' + raw.subarray(2, 22).toString('hex');
				if (agentArgs.toLowerCase() !== lock_args.toLowerCase()) continue;
				capCount++;
				if (raw[1] === 1) chainBackedCaps++;
			}
		}

		// 4. Badges (proof of past work).
		let badgeCount = 0;
		if (data.badges) {
			badgeCount = data.badges.objects.filter(
				(c) => c.output.lock.args.toLowerCase() === lock_args.toLowerCase(),
			).length;
		}

		// Reputation (0-40): ratio weighted by volume, bonus for proof chain.
		const totalJobs = jobsCompleted + jobsAbandoned;
		const ratio = totalJobs > 0 ? jobsCompleted / totalJobs : 0;
		const volumeMultiplier = Math.min(totalJobs / 10, 1); // full credit at 10+ jobs
		let reputationScore = Math.round(ratio * 30 * volumeMultiplier);
		if (hasProofChain) reputationScore += 10; // bonus for V1 verifiable chain
		reputationScore = Math.min(reputationScore, 40);

		// Capabilities (0-25): count * proof strength.
		const attestationCaps = capCount - chainBackedCaps;
		const capabilityScore = Math.min(attestationCaps * 5 + chainBackedCaps * 10, 25);

		// Track record (0-20): badge count, diminishing returns.
		const trackRecordScore = Math.min(Math.round(Math.sqrt(badgeCount) * 10), 20);

		// Solvency (0-15): log-scale on-chain balance.
		let balanceCkb = 0;
		try {
			const shannons = await getBalanceByLock(lock_args);
			balanceCkb = Number(shannons / 100_000_000n);
		} catch { /* balance lookup failed — use 0 */ }
		const solvencyScore = Math.min(Math.round(Math.log10(Math.max(balanceCkb, 1)) * 5), 15);

		const composite = reputationScore + capabilityScore + trackRecordScore + solvencyScore;

		const trustLevel =
			composite >= 80 ? 'excellent' :
			composite >= 60 ? 'good' :
			composite >= 40 ? 'developing' :
			composite >= 20 ? 'new' :
			'unknown';

		res.json({
			lock_args,
			trust_score: composite,
			trust_level: trustLevel,
			breakdown: {
				reputation: {
					score: reputationScore,
					max: 40,
					jobs_completed: jobsCompleted,
					jobs_abandoned: jobsAbandoned,
					has_proof_chain: hasProofChain,
				},
				capabilities: {
					score: capabilityScore,
					max: 25,
					total: capCount,
					chain_backed: chainBackedCaps,
					attestation: attestationCaps,
				},
				track_record: {
					score: trackRecordScore,
					max: 20,
					badges: badgeCount,
				},
				solvency: {
					score: solvencyScore,
					max: 15,
				},
			},
		});
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

export default router;
