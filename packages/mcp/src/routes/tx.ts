// TX template, submit, and status endpoints for external agents.
//
// POST /tx/template: build unsigned TX + signing message.
// POST /tx/submit: inject signature + broadcast.
// GET  /tx/status/:tx_hash: check TX status.

import { Router, Request, Response } from 'express';
import { getTransaction, sendTransaction } from '../ckb.js';
import {
	buildSpawnAgent,
	buildCreateReputation,
	buildPostJob,
	buildReserveJob,
	buildClaimJob,
	buildCompleteJob,
	buildTransfer,
	injectSignature,
	UnsignedTx,
} from '../tx-builder.js';

const router = Router();

// Intent name → builder function.
const intentBuilders: Record<string, (lockArgs: string, params: Record<string, unknown>) => Promise<{ tx: UnsignedTx; tx_hash: string; signing_message: string }>> = {
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	spawn_agent: (la, p) => buildSpawnAgent(la, p as any),
	create_reputation: (la) => buildCreateReputation(la),
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	post_job: (la, p) => buildPostJob(la, p as any),
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	reserve_job: (la, p) => buildReserveJob(la, p as any),
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	claim_job: (la, p) => buildClaimJob(la, p as any),
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	complete_job: (la, p) => buildCompleteJob(la, p as any),
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	transfer: (la, p) => buildTransfer(la, p as any),
};

// POST /tx/template: build unsigned transaction + signing message.
router.post('/template', async (req: Request, res: Response) => {
	try {
		const { intent, lock_args, params } = req.body;

		if (!intent || typeof intent !== 'string') {
			res.status(400).json({ error: 'missing or invalid "intent" field' });
			return;
		}
		if (!lock_args || typeof lock_args !== 'string') {
			res.status(400).json({ error: 'missing or invalid "lock_args" field' });
			return;
		}

		const builder = intentBuilders[intent];
		if (!builder) {
			res.status(400).json({
				error: `unknown intent: ${intent}`,
				supported: Object.keys(intentBuilders),
			});
			return;
		}

		const result = await builder(lock_args, params ?? {});

		res.json({
			tx: result.tx,
			tx_hash: result.tx_hash,
			signing_message: result.signing_message,
			witness_index: 0,
			instructions: 'Sign the signing_message with secp256k1, then POST to /tx/submit.',
		});
	} catch (err) {
		const message = err instanceof Error ? err.message : 'unknown error';
		console.error('tx/template error:', message);
		res.status(500).json({ error: message });
	}
});

// POST /tx/submit: inject signature and broadcast.
router.post('/submit', async (req: Request, res: Response) => {
	try {
		const { tx, signature } = req.body;

		if (!tx || typeof tx !== 'object') {
			res.status(400).json({ error: 'missing or invalid "tx" field' });
			return;
		}
		if (!signature || typeof signature !== 'string') {
			res.status(400).json({ error: 'missing or invalid "signature" field (hex string)' });
			return;
		}

		const signedTx = injectSignature(tx as UnsignedTx, signature);
		const txHash = await sendTransaction(signedTx);

		res.json({ tx_hash: txHash, status: 'submitted' });
	} catch (err) {
		const message = err instanceof Error ? err.message : 'unknown error';
		console.error('tx/submit error:', message);
		res.status(500).json({ error: message });
	}
});

// GET /tx/status/:tx_hash: check transaction status.
router.get('/status/:tx_hash', async (req: Request, res: Response) => {
	try {
		const { tx_hash } = req.params;
		const result = await getTransaction(tx_hash) as { tx_status?: { status?: string } } | null;

		const status = result?.tx_status?.status ?? 'unknown';
		res.json({ tx_hash, status });
	} catch (err) {
		const message = err instanceof Error ? err.message : 'unknown error';
		console.error('tx/status error:', message);
		res.status(500).json({ error: message });
	}
});

export default router;
