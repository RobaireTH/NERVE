import { Router } from 'express';
import fs from 'fs';
import path from 'path';
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
	ensureDirectPeerFromRpcUrl,
	waitForChannelReadyByPeer,
} from '../fiber.js';
import { getCellsByScript, Script } from '../ckb.js';
import { parseAgentCell } from './agents.js';

const router = Router();

const AGENT_TYPE_CODE_HASH = process.env.AGENT_IDENTITY_TYPE_CODE_HASH ?? '';
const FIBER_AGENT_MAP_PATH = process.env.FIBER_AGENT_MAP_PATH ?? path.resolve(process.cwd(), '.fiber-agents.json');

export interface FiberAgentEntry {
	lock_args: string;
	node_id: string;
	rpc_url?: string;
	notes?: string;
	updated_at: string;
}

export interface FiberAgentPaymentResult {
	payment_hash: string;
	status: string;
	fee_shannons: number;
	target_pubkey: string;
	agent_pubkey: string | null;
	fiber_node_id: string | null;
	fiber_rpc_url: string | null;
	used_mapping: boolean;
	used_invoice: boolean;
}

export function readFiberAgentMap(): Record<string, FiberAgentEntry> {
	try {
		if (!fs.existsSync(FIBER_AGENT_MAP_PATH)) return {};
		return JSON.parse(fs.readFileSync(FIBER_AGENT_MAP_PATH, 'utf8')) as Record<string, FiberAgentEntry>;
	} catch {
		return {};
	}
}

function writeFiberAgentMap(map: Record<string, FiberAgentEntry>): void {
	fs.writeFileSync(FIBER_AGENT_MAP_PATH, JSON.stringify(map, null, 2) + '\n');
}

export async function remoteRpcCall<T>(rpcUrl: string, method: string, params: unknown[]): Promise<T> {
	const resp = await fetch(rpcUrl, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
	});
	const json = await resp.json() as { result?: T, error?: { message?: string } };
	if (json.error || json.result === undefined) {
		throw new Error(json.error?.message ?? `${method} failed`);
	}
	return json.result;
}

async function createRemoteInvoice(rpcUrl: string, amountCkb: number, description: string): Promise<string> {
	const amount = `0x${BigInt(Math.round(amountCkb * 100_000_000)).toString(16)}`;
	const result = await remoteRpcCall<{ invoice_address?: string }>(rpcUrl, 'new_invoice', [{
		amount,
		description,
		currency: process.env.FIBER_CURRENCY ?? 'Fibt',
		expiry: '0xe10',
		final_expiry_delta: '0x5265c00',
	}]);
	if (!result.invoice_address) {
		throw new Error('remote invoice creation failed');
	}
	return result.invoice_address;
}

async function acceptRemoteChannel(rpcUrl: string, temporaryChannelId: string, fundingCkb: number): Promise<string> {
	const fundingAmount = `0x${BigInt(Math.round(fundingCkb * 100_000_000)).toString(16)}`;
	const result = await remoteRpcCall<{ channel_id?: string }>(rpcUrl, 'accept_channel', [{
		temporary_channel_id: temporaryChannelId,
		funding_amount: fundingAmount,
		shutdown_script: null,
		max_tlc_number_in_flight: '0x7d',
		tlc_min_value: '0x0',
		tlc_fee_proportional_millionths: '0x3e8',
		tlc_expiry_delta: '0xdbba00',
	}]);
	if (!result.channel_id) {
		throw new Error('remote channel accept failed');
	}
	return result.channel_id;
}

// GET /fiber/node: Fiber node info (node_id, addresses, channel count).
router.get('/node', async (_req, res) => {
	try {
		const info = await nodeInfo();
		res.json(info);
	} catch (e) {
		console.error('fiber route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// POST /fiber/peers: Connect to a peer by multiaddr.
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

// GET /fiber/channels: List open channels.
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

// POST /fiber/channels: Open a channel with a connected peer.
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

// DELETE /fiber/channels/:channel_id: Cooperatively close a channel.
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

// POST /fiber/invoice: Create a payment invoice.
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

// POST /fiber/hold-invoice: Create a hold invoice (escrow) with a pre-determined payment_hash.
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

// POST /fiber/settle: Settle a hold invoice by revealing the preimage.
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

// GET /fiber/invoice/:payment_hash: Get invoice status by payment_hash.
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

// POST /fiber/pay: Send a payment by invoice or keysend.
//
// Option A: Pay by invoice (recommended).
//   Body: { "invoice": "fibt1..." }
//
// Option B: Keysend (spontaneous, no invoice).
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

// GET /fiber/ready: Quick readiness check for the Fiber payment layer.
// Returns { ready: true/false } so agents can preflight before payment operations.
router.get('/ready', async (_req, res) => {
	try {
		const ready = await isNodeReady();
		res.json({ ready });
	} catch {
		res.json({ ready: false });
	}
});

// GET /fiber/agents/:lock_args: read Fiber node mapping for an agent lock_args.
router.get('/agents/:lock_args', async (req, res) => {
	const { lock_args } = req.params;
	const map = readFiberAgentMap();
	const entry = map[lock_args.toLowerCase()];
	if (!entry) {
		res.status(404).json({ error: 'no Fiber mapping found for this lock_args' });
		return;
	}
	res.json(entry);
});

// POST /fiber/agents: register lock_args -> Fiber node id mapping.
router.post('/agents', async (req, res) => {
	const { lock_args, node_id, rpc_url, notes } = req.body as {
		lock_args?: string;
		node_id?: string;
		rpc_url?: string;
		notes?: string;
	};
	if (!lock_args || !node_id) {
		res.status(400).json({ error: 'lock_args and node_id are required' });
		return;
	}
	const map = readFiberAgentMap();
	const entry: FiberAgentEntry = {
		lock_args: lock_args.toLowerCase(),
		node_id,
		rpc_url,
		notes,
		updated_at: new Date().toISOString(),
	};
	map[entry.lock_args] = entry;
	writeFiberAgentMap(map);
	res.status(201).json(entry);
});

export async function payAgentByLockArgs(lockArgs: string, amountCkb: number, description?: string): Promise<FiberAgentPaymentResult> {
	if (!AGENT_TYPE_CODE_HASH) {
		throw new Error('AGENT_IDENTITY_TYPE_CODE_HASH not configured');
	}

	const normalizedLockArgs = lockArgs.toLowerCase();
	const map = readFiberAgentMap();
	const mapped = map[normalizedLockArgs];

	let targetPubkey = mapped?.node_id;
	let agentPubkey: string | undefined;

	if (!targetPubkey) {
		const script: Script = {
			code_hash: AGENT_TYPE_CODE_HASH,
			hash_type: 'data1',
			args: '0x',
		};
		const result = await getCellsByScript(script, 'type', 200);
		const match = result.objects.find(
			(c) => c.output.lock.args.toLowerCase() === normalizedLockArgs,
		);
		if (!match) {
			throw new Error('no agent identity cell found for this lock_args');
		}
		const agent = parseAgentCell(match.output_data, lockArgs);
		if (!agent) {
			throw new Error('cell is not a valid agent identity cell');
		}
		agentPubkey = agent.pubkey;
		targetPubkey = agent.pubkey;
	}

	const payOnce = async () => mapped?.rpc_url
		? sendPayment({ invoice: await createRemoteInvoice(mapped.rpc_url, amountCkb, description ?? '') })
		: sendPayment({ targetPubkey, amountCkb, description });

	let payment;
	try {
		payment = await payOnce();
	} catch (e) {
		const message = e instanceof Error ? e.message : String(e);
		if (!mapped?.rpc_url || !mapped?.node_id || !/no path found|Insufficient balance/i.test(message)) {
			throw e;
		}
		const peerId = await ensureDirectPeerFromRpcUrl(mapped.rpc_url, mapped.node_id);
		try {
			const opened = await openChannel(peerId, 100, true);
			await acceptRemoteChannel(mapped.rpc_url, opened.temporary_channel_id, 100);
		} catch {
			// ignore if a usable channel already exists or another open is already in progress
		}
		await waitForChannelReadyByPeer(peerId, amountCkb, 180000);
		let lastError: unknown = e;
		for (let i = 0; i < 8; i++) {
			try {
				payment = await payOnce();
				lastError = null;
				break;
			} catch (retryError) {
				lastError = retryError;
				await new Promise((resolve) => setTimeout(resolve, 4000));
			}
		}
		if (!payment && lastError) throw lastError;
	}

	if (!payment) {
		throw new Error('fiber payment failed after retries');
	}

	return {
		payment_hash: payment.payment_hash,
		status: payment.status,
		fee_shannons: payment.fee,
		target_pubkey: targetPubkey,
		agent_pubkey: agentPubkey ?? null,
		fiber_node_id: mapped?.node_id ?? null,
		fiber_rpc_url: mapped?.rpc_url ?? null,
		used_mapping: Boolean(mapped),
		used_invoice: Boolean(mapped?.rpc_url),
	};
}

// POST /fiber/pay-agent: Look up agent pubkey by lock_args and keysend payment.
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

	try {
		const payment = await payAgentByLockArgs(lock_args, amount_ckb, description);
		res.json(payment);
	} catch (e) {
		console.error('fiber route error:', e);
		const message = e instanceof Error ? e.message : 'upstream request failed';
		if (message === 'AGENT_IDENTITY_TYPE_CODE_HASH not configured') {
			res.status(503).json({ error: message });
			return;
		}
		if (message === 'no agent identity cell found for this lock_args') {
			res.status(404).json({ error: message });
			return;
		}
		if (message === 'cell is not a valid agent identity cell') {
			res.status(422).json({ error: message });
			return;
		}
		res.status(502).json({ error: 'upstream request failed', details: message });
	}
});

export default router;
