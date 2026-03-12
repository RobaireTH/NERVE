import { Router } from 'express';
import {
	nodeInfo,
	connectPeer,
	openChannel,
	listChannels,
	shutdownChannel,
	newInvoice,
	sendPayment,
	shannonsToNumber,
} from '../fiber.js';

const router = Router();

// GET /fiber/node — Fiber node info (node_id, addresses, channel count).
router.get('/node', async (_req, res) => {
	try {
		const info = await nodeInfo();
		res.json(info);
	} catch (e) {
		res.status(502).json({ error: String(e) });
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
		res.status(502).json({ error: String(e) });
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
		res.status(502).json({ error: String(e) });
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
		res.status(502).json({ error: String(e) });
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
		res.status(502).json({ error: String(e) });
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
		res.status(502).json({ error: String(e) });
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
		res.status(502).json({ error: String(e) });
	}
});

export default router;
