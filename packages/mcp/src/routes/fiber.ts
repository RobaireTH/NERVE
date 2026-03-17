import { Router } from 'express';
import {
	nodeInfo,
	connectPeer,
	openChannel,
	listChannels,
	shutdownChannel,
	newInvoice,
	newHoldInvoice,
	settleInvoice,
	getInvoice,
	sendPayment,
	shannonsToNumber,
	isNodeReady,
} from '../fiber.js';
import { getCellsByScript, Script } from '../ckb.js';
import { parseAgentCell } from './agents.js';

const router = Router();

const AGENT_TYPE_CODE_HASH = process.env.AGENT_IDENTITY_TYPE_CODE_HASH ?? '';

// GET /fiber/node — Fiber node info (node_id, addresses, channel count).
router.get('/node', async (_req, res) => {
	try {
		const info = await nodeInfo();
		res.json(info);
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /fiber/peers — Connect to a peer by multiaddr.
// Body: { "peer_address": "/ip4/.../tcp/8228/p2p/<peer_id>" }
router.post('/peers', async (req, res) => {
	const { peer_address } = req.body as { peer_address?: string };
	if (!peer_address) {
		res.status(400).json({ error: 'peer_address is required' });
		return;
	}
	try {
		await connectPeer(peer_address);
		res.json({ ok: true, peer_address });
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /fiber/channels — List open channels.
// Query: ?peer_id=<optional>
router.get('/channels', async (req, res) => {
	const peerId = req.query.peer_id as string | undefined;
	try {
		const result = await listChannels(peerId);
		const channels = result.channels.map((c) => ({
			...c,
			local_balance_ckb: shannonsToNumber(c.local_balance),
			remote_balance_ckb: shannonsToNumber(c.remote_balance),
		}));
		res.json({ channels, count: channels.length });
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /fiber/channels — Open a channel with a connected peer.
// Body: { "peer_id": "0x...", "funding_ckb": 100, "public": true }
// The peer must already be connected (POST /fiber/peers first).
router.post('/channels', async (req, res) => {
	const { peer_id, funding_ckb, public: isPublic = true } = req.body as {
		peer_id?: string;
		funding_ckb?: number;
		public?: boolean;
	};
	if (!peer_id || funding_ckb === undefined) {
		res.status(400).json({ error: 'peer_id and funding_ckb are required' });
		return;
	}
	try {
		const result = await openChannel(peer_id, funding_ckb, isPublic);
		res.status(201).json({
			temporary_channel_id: result.temporary_channel_id,
			peer_id,
			funding_ckb,
		});
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// DELETE /fiber/channels/:channel_id — Cooperatively close a channel.
// Query: ?force=true for uncooperative close.
router.delete('/channels/:channel_id', async (req, res) => {
	const { channel_id } = req.params;
	const force = req.query.force === 'true';
	try {
		await shutdownChannel(channel_id, force);
		res.json({ ok: true, channel_id, force });
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /fiber/invoice — Create a payment invoice.
// Body: { "amount_ckb": 5, "description": "payment for job X", "expiry_seconds": 3600 }
router.post('/invoice', async (req, res) => {
	const { amount_ckb, description = '', expiry_seconds = 3600 } = req.body as {
		amount_ckb?: number;
		description?: string;
		expiry_seconds?: number;
	};
	if (amount_ckb === undefined) {
		res.status(400).json({ error: 'amount_ckb is required' });
		return;
	}
	try {
		const invoice = await newInvoice(amount_ckb, description, expiry_seconds);
		res.json({
			invoice_address: invoice.invoice_address,
			amount_ckb,
			payment_hash: invoice.invoice.data.payment_hash,
		});
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /fiber/hold-invoice — Create a hold invoice (escrow) with a pre-determined payment_hash.
// Body: { "amount_ckb": 5, "payment_hash": "0x...", "description": "escrow for job X" }
router.post('/hold-invoice', async (req, res) => {
	const { amount_ckb, payment_hash, description = '' } = req.body as {
		amount_ckb?: number;
		payment_hash?: string;
		description?: string;
	};
	if (amount_ckb === undefined || !payment_hash) {
		res.status(400).json({ error: 'amount_ckb and payment_hash are required' });
		return;
	}
	try {
		const invoice = await newHoldInvoice(amount_ckb, payment_hash, description);
		res.json({
			invoice_address: invoice.invoice_address,
			amount_ckb,
			payment_hash: invoice.invoice.data.payment_hash,
		});
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /fiber/settle — Settle a hold invoice by revealing the preimage.
// Body: { "payment_hash": "0x...", "preimage": "0x..." }
router.post('/settle', async (req, res) => {
	const { payment_hash, preimage } = req.body as {
		payment_hash?: string;
		preimage?: string;
	};
	if (!payment_hash || !preimage) {
		res.status(400).json({ error: 'payment_hash and preimage are required' });
		return;
	}
	try {
		await settleInvoice(payment_hash, preimage);
		res.json({ ok: true, payment_hash });
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /fiber/invoice/:payment_hash — Get invoice status by payment_hash.
router.get('/invoice/:payment_hash', async (req, res) => {
	const { payment_hash } = req.params;
	try {
		const invoice = await getInvoice(payment_hash);
		res.json({
			invoice_address: invoice.invoice_address,
			payment_hash,
			amount: invoice.invoice.amount,
			currency: invoice.invoice.currency,
		});
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /fiber/pay — Send a payment by invoice or keysend.
//
// Option A — Pay by invoice (recommended):
//   Body: { "invoice": "fibt1..." }
//
// Option B — Keysend (spontaneous, no invoice):
//   Body: { "target_pubkey": "0x...", "amount_ckb": 5 }
router.post('/pay', async (req, res) => {
	const { invoice, target_pubkey, amount_ckb, description } = req.body as {
		invoice?: string;
		target_pubkey?: string;
		amount_ckb?: number;
		description?: string;
	};

	if (!invoice && (!target_pubkey || amount_ckb === undefined)) {
		res.status(400).json({
			error: 'Provide either invoice or (target_pubkey + amount_ckb)',
		});
		return;
	}

	try {
		const result = await sendPayment({ invoice, targetPubkey: target_pubkey, amountCkb: amount_ckb, description });
		res.json({
			payment_hash: result.payment_hash,
			status: result.status,
			fee_shannons: result.fee,
		});
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /fiber/ready — Quick readiness check for the Fiber payment layer.
// Returns { ready: true/false } so agents can preflight before payment operations.
router.get('/ready', async (_req, res) => {
	try {
		const ready = await isNodeReady();
		res.json({ ready });
	} catch {
		res.json({ ready: false });
	}
});

// POST /fiber/pay-agent — Look up agent pubkey by lock_args and keysend payment.
router.post('/pay-agent', async (req, res) => {
	const { lock_args, amount_ckb, description } = req.body as {
		lock_args?: string;
		amount_ckb?: number;
		description?: string;
	};

	if (!lock_args || amount_ckb === undefined) {
		res.status(400).json({ error: 'lock_args and amount_ckb are required' });
		return;
	}
	if (!AGENT_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'AGENT_IDENTITY_TYPE_CODE_HASH not configured' });
		return;
	}

	try {
		// Look up identity cells and find the matching agent.
		const script: Script = {
			code_hash: AGENT_TYPE_CODE_HASH,
			hash_type: 'data1',
			args: '0x',
		};
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

		// Keysend to the agent's pubkey.
		const payment = await sendPayment({ targetPubkey: agent.pubkey, amountCkb: amount_ckb, description });
		res.json({
			payment_hash: payment.payment_hash,
			status: payment.status,
			fee_shannons: payment.fee,
			agent_pubkey: agent.pubkey,
		});
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

export default router;
