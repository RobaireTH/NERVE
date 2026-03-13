import { Router } from 'express';
import { getCellsByScript, getBalanceByLock, getTipBlockNumber, Script } from '../ckb.js';

const router = Router();

// GET /chain/height — current tip block number.
router.get('/height', async (_req, res) => {
	try {
		const height = await getTipBlockNumber();
		res.json({ block_number: height.toString() });
	} catch (e) {
		console.error('chain route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /chain/cells?code_hash=&hash_type=&args=&script_type= — scan cells by script.
router.get('/cells', async (req, res) => {
	const { code_hash, hash_type, args, script_type } = req.query as Record<string, string>;
	if (!code_hash || !hash_type || !args || !script_type) {
		res.status(400).json({ error: 'code_hash, hash_type, args, script_type are required' });
		return;
	}
	if (script_type !== 'lock' && script_type !== 'type') {
		res.status(400).json({ error: 'script_type must be lock or type' });
		return;
	}
	try {
		const script: Script = { code_hash, hash_type, args };
		const result = await getCellsByScript(script, script_type as 'lock' | 'type');
		res.json(result);
	} catch (e) {
		console.error('chain route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /chain/balance/:lock_args — CKB balance for a secp256k1-blake2b lock.
router.get('/balance/:lock_args', async (req, res) => {
	const { lock_args } = req.params;
	try {
		const shannons = await getBalanceByLock(lock_args);
		res.json({
			lock_args,
			balance_shannons: shannons.toString(),
			balance_ckb: Number(shannons) / 1e8,
		});
	} catch (e) {
		console.error('chain route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

export default router;
