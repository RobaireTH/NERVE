import { Router } from 'express';
import blake2b from 'blake2b';
import { getCellsByScript, Script } from '../ckb.js';

const router = Router();

const AGENT_TYPE_CODE_HASH = process.env.AGENT_IDENTITY_TYPE_CODE_HASH ?? '';
const REP_TYPE_CODE_HASH = process.env.REPUTATION_TYPE_CODE_HASH ?? '';
const DOB_BADGE_CODE_HASH = process.env.DOB_BADGE_CODE_HASH ?? '';
const CAP_NFT_TYPE_CODE_HASH = process.env.CAP_NFT_TYPE_CODE_HASH ?? '';

// ─── Agent identity cell data layout (50 bytes) ──────────────────────────────
// [0]      version: u8
// [1..34]  compressed_pubkey: [u8; 33]
// [34..42] spending_limit: u64 LE
// [42..50] daily_limit: u64 LE

interface AgentInfo {
	lock_args: string;
	pubkey: string;
	spending_limit_ckb: number;
	daily_limit_ckb: number;
	version: number;
	parent_lock_args?: string;
	revenue_share_bps?: number;
}

function parseAgentCell(outputData: string, lockArgs: string): AgentInfo | null {
	if (!outputData || outputData === '0x' || outputData.length < 2 + 100) return null;
	const raw = Buffer.from(outputData.replace('0x', ''), 'hex');
	if (raw.length < 50) return null;
	const version = raw[0];
	if (version > 1) return null;
	const pubkey = '0x' + raw.subarray(1, 34).toString('hex');
	const spendingLimit = raw.readBigUInt64LE(34);
	const dailyLimit = raw.readBigUInt64LE(42);
	const info: AgentInfo = {
		lock_args: lockArgs,
		pubkey,
		spending_limit_ckb: Number(spendingLimit) / 1e8,
		daily_limit_ckb: Number(dailyLimit) / 1e8,
		version,
	};
	// V1 identity: parse parent delegation fields.
	if (version >= 1 && raw.length >= 72) {
		info.parent_lock_args = '0x' + raw.subarray(50, 70).toString('hex');
		info.revenue_share_bps = raw.readUInt16LE(70);
	}
	return info;
}

// ─── Reputation cell data layout (46 bytes) ───────────────────────────────────
// [0]      version: u8
// [1]      pending_type: u8
// [2..10]  jobs_completed: u64 LE
// [10..18] jobs_abandoned: u64 LE
// [18..26] pending_expires_at: u64 LE
// [26..46] agent_lock_args: [u8; 20]

interface ReputationInfo {
	agent_lock_args: string;
	jobs_completed: number;
	jobs_abandoned: number;
	pending_type: number;
	pending_expires_at: string;
	version: number;
	proof_root?: string;
	pending_settlement_hash?: string;
}

function parseReputationCell(outputData: string): ReputationInfo | null {
	if (!outputData || outputData === '0x' || outputData.length < 2 + 92) return null;
	const raw = Buffer.from(outputData.replace('0x', ''), 'hex');
	if (raw.length < 46) return null;
	const version = raw[0];
	if (version > 1) return null;
	const pendingType = raw[1];
	const completed = raw.readBigUInt64LE(2);
	const abandoned = raw.readBigUInt64LE(10);
	const expiresAt = raw.readBigUInt64LE(18);
	const agentLockArgs = '0x' + raw.subarray(26, 46).toString('hex');
	const info: ReputationInfo = {
		agent_lock_args: agentLockArgs,
		jobs_completed: Number(completed),
		jobs_abandoned: Number(abandoned),
		pending_type: pendingType,
		pending_expires_at: expiresAt.toString(),
		version,
	};
	// V1: parse proof_root and pending_settlement_hash.
	if (version >= 1 && raw.length >= 110) {
		info.proof_root = '0x' + raw.subarray(46, 78).toString('hex');
		info.pending_settlement_hash = '0x' + raw.subarray(78, 110).toString('hex');
	}
	return info;
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
		if (!rep || rep.version < 1 || !rep.proof_root) {
			res.status(422).json({ error: 'reputation cell is not V1 (no proof_root)' });
			return;
		}

		// Parse settlement hashes.
		const hashes = hashesParam.split(',').map((h) => h.trim());
		const hashBuffers = hashes.map((h) => Buffer.from(h.replace('0x', ''), 'hex'));

		// Replay the hash chain from genesis (all zeros).
		let root = Buffer.alloc(32);
		for (const sh of hashBuffers) {
			const preimage = Buffer.concat([root, sh]);
			root = ckbBlake2b(preimage);
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
				// Only include v1 identities whose parent_lock_args matches.
				if (
					agent.version < 1 ||
					!agent.parent_lock_args ||
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

export default router;
