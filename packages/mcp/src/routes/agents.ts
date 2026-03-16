import { Router } from 'express';
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
}

function parseAgentCell(outputData: string, lockArgs: string): AgentInfo | null {
	if (!outputData || outputData === '0x' || outputData.length < 2 + 100) return null;
	const raw = Buffer.from(outputData.replace('0x', ''), 'hex');
	if (raw.length < 50) return null;
	if (raw[0] !== 0) return null;
	const pubkey = '0x' + raw.subarray(1, 34).toString('hex');
	const spendingLimit = raw.readBigUInt64LE(34);
	const dailyLimit = raw.readBigUInt64LE(42);
	return {
		lock_args: lockArgs,
		pubkey,
		spending_limit_ckb: Number(spendingLimit) / 1e8,
		daily_limit_ckb: Number(dailyLimit) / 1e8,
	};
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
}

function parseReputationCell(outputData: string): ReputationInfo | null {
	if (!outputData || outputData === '0x' || outputData.length < 2 + 92) return null;
	const raw = Buffer.from(outputData.replace('0x', ''), 'hex');
	if (raw.length < 46) return null;
	if (raw[0] !== 0) return null;
	const pendingType = raw[1];
	const completed = raw.readBigUInt64LE(2);
	const abandoned = raw.readBigUInt64LE(10);
	const expiresAt = raw.readBigUInt64LE(18);
	const agentLockArgs = '0x' + raw.subarray(26, 46).toString('hex');
	return {
		agent_lock_args: agentLockArgs,
		jobs_completed: Number(completed),
		jobs_abandoned: Number(abandoned),
		pending_type: pendingType,
		pending_expires_at: expiresAt.toString(),
	};
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
				return {
					out_point: c.out_point,
					capability_hash: '0x' + raw.subarray(22, 54).toString('hex'),
				};
			});
		res.json({ capabilities, count: capabilities.length });
	} catch (e) {
		console.error('agents route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

export default router;
