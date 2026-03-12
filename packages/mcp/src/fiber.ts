// Fiber Network JSON-RPC client.
//
// The Fiber node exposes a namespaced JSON-RPC API:
//   info_node_info, peer_connect_peer, channel_open_channel,
//   channel_list_channels, channel_shutdown_channel,
//   invoice_new_invoice, payment_send_payment
//
// Amounts are in shannons (u128). All hex values are 0x-prefixed.

const FIBER_RPC_URL = process.env.FIBER_RPC_URL ?? 'http://localhost:8227';

let _id = 1;

async function fiberRpc<T>(method: string, params: Record<string, unknown>): Promise<T> {
	const res = await fetch(FIBER_RPC_URL, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ jsonrpc: '2.0', id: _id++, method, params }),
	});
	const json = (await res.json()) as { result?: T; error?: { message: string } };
	if (json.error) throw new Error(`Fiber RPC ${method}: ${json.error.message}`);
	if (json.result === undefined) throw new Error(`Fiber RPC ${method}: empty result`);
	return json.result;
}

export const CKB_TO_SHANNONS = 100_000_000n;

export function ckbToShannons(ckb: number): bigint {
	return BigInt(Math.round(ckb * 1e8));
}

export function shannonsToNumber(shannons: bigint | number | string): number {
	return Number(BigInt(shannons)) / 1e8;
}

// ─── Types ─────────────────────────────────────────────────────────────────────

export interface FiberNodeInfo {
	version: string;
	node_id: string;
	addresses: string[];
	channel_count: number;
	pending_channel_count: number;
	peers_count: number;
}

export interface FiberChannel {
	channel_id: string;
	is_public: boolean;
	peer_id: string;
	state: string;
	local_balance: number;
	remote_balance: number;
	enabled: boolean;
}

export interface FiberInvoice {
	invoice_address: string;
	invoice: {
		currency: string;
		amount: number;
		data: { payment_hash: string };
	};
}

export interface FiberPaymentResult {
	payment_hash: string;
	status: string;
	fee: number;
}

// ─── API methods ───────────────────────────────────────────────────────────────

export async function nodeInfo(): Promise<FiberNodeInfo> {
	return fiberRpc<FiberNodeInfo>('info_node_info', {});
}

export async function connectPeer(peerAddress: string, save = true): Promise<void> {
	await fiberRpc('peer_connect_peer', { address: peerAddress, save });
}

export async function openChannel(
	peerId: string,
	fundingCkb: number,
	isPublic = true,
): Promise<{ temporary_channel_id: string }> {
	const fundingAmount = ckbToShannons(fundingCkb);
	return fiberRpc('channel_open_channel', {
		peer_id: peerId,
		funding_amount: Number(fundingAmount),
		public: isPublic,
		funding_udt_type_script: null,
		shutdown_script: null,
		tlc_expiry_delta: 14_400_000,   // 4 hours in ms
		tlc_min_value: 0,
		tlc_fee_proportional_millionths: 1000,
		max_tlc_number_in_flight: 125,
	});
}

export async function listChannels(peerId?: string): Promise<{ channels: FiberChannel[] }> {
	return fiberRpc('channel_list_channels', {
		peer_id: peerId ?? null,
		include_closed: false,
	});
}

export async function shutdownChannel(
	channelId: string,
	force = false,
): Promise<void> {
	await fiberRpc('channel_shutdown_channel', {
		channel_id: channelId,
		close_script: null,
		fee_rate: 1000,
		force,
	});
}

export async function newInvoice(
	amountCkb: number,
	description: string,
	expirySeconds = 3600,
): Promise<FiberInvoice> {
	const currency = process.env.FIBER_CURRENCY ?? 'Fibt'; // Fibt = testnet
	return fiberRpc('invoice_new_invoice', {
		amount: Number(ckbToShannons(amountCkb)),
		description,
		currency,
		payment_preimage: null,
		payment_hash: null,
		expiry: expirySeconds,
		final_expiry_delta: 86_400_000,
	});
}

// Send payment by invoice string (preferred) or keysend (target_pubkey + amount).
export async function sendPayment(opts: {
	invoice?: string;
	targetPubkey?: string;
	amountCkb?: number;
	description?: string;
}): Promise<FiberPaymentResult> {
	const params: Record<string, unknown> = {
		timeout: 300,
		max_fee_amount: Number(ckbToShannons(0.01)), // 0.01 CKB max fee
		max_parts: 1,
		keysend: !opts.invoice,
		dry_run: false,
		hop_hints: [],
	};

	if (opts.invoice) {
		params.invoice = opts.invoice;
	} else {
		if (!opts.targetPubkey || opts.amountCkb === undefined) {
			throw new Error('Either invoice or (targetPubkey + amountCkb) is required');
		}
		params.target_pubkey = opts.targetPubkey;
		params.amount = Number(ckbToShannons(opts.amountCkb));
	}

	return fiberRpc('payment_send_payment', params);
}
