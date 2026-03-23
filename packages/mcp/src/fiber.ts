// Fiber Network client: backed by @fiber-pay/sdk FiberRpcClient.
//
// Wraps the SDK's typed RPC client and re-exports the same function signatures
// used by routes/fiber.ts for backward compatibility. Gains: proper error types
// (FiberRpcError), typed params/results, wait helpers.

import { FiberRpcClient, FiberRpcError, ckbToShannons as sdkCkbToShannons, buildMultiaddrFromNodeId, buildMultiaddrFromRpcUrl, nodeIdToPeerId } from '@fiber-pay/sdk';

const FIBER_RPC_URL = process.env.FIBER_RPC_URL ?? 'http://localhost:8227';

const client = new FiberRpcClient({ url: FIBER_RPC_URL });

export const CKB_TO_SHANNONS = 100_000_000n;

export function ckbToShannons(ckb: number): string {
	return sdkCkbToShannons(ckb);
}

export function toHexUint(value: bigint | number | string): string {
	return `0x${BigInt(value).toString(16)}`;
}

export function shannonsToNumber(shannons: bigint | number | string): number {
	return Number(BigInt(shannons)) / 1e8;
}

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

export interface FiberHoldInvoice {
	invoice_address: string;
	invoice: {
		currency: string;
		amount: number;
		data: { payment_hash: string };
	};
}

export { FiberRpcError };

export async function nodeInfo(): Promise<FiberNodeInfo> {
	try {
		return await client.call<FiberNodeInfo>('node_info', [{}]);
	} catch (e) {
		if (e instanceof FiberRpcError) throw e;
		throw new Error(`Fiber RPC node_info: ${(e as Error).message}`);
	}
}

export async function connectPeer(peerAddress: string, save = true): Promise<void> {
	let address = peerAddress;
	if (/\/p2p\/0[0-9a-f]{65}$/i.test(peerAddress)) {
		const [base, nodeId] = peerAddress.split('/p2p/');
		address = await buildMultiaddrFromNodeId(base, nodeId);
	}
	await client.call('connect_peer', [{ address, save }]);
}

export async function ensureDirectPeerFromRpcUrl(rpcUrl: string, nodeId: string): Promise<string> {
	const peerId = await nodeIdToPeerId(nodeId);
	const address = buildMultiaddrFromRpcUrl(rpcUrl, nodeId);
	await connectPeer(address, true);
	return peerId;
}

export async function waitForChannelReadyByPeer(peerId: string, minLocalCkb = 0, timeoutMs = 120000): Promise<boolean> {
	const start = Date.now();
	while (Date.now() - start < timeoutMs) {
		const result = await listChannels(peerId);
		if (result.channels.some((c: any) => {
			const stateName = c.state?.state_name ?? c.state;
			const local = typeof c.local_balance === 'string'
				? Number(BigInt(c.local_balance)) / 1e8
				: Number(c.local_balance ?? 0) / (Number(c.local_balance ?? 0) > 1e6 ? 1e8 : 1);
			return (stateName === 'CHANNEL_READY' || stateName === 'ChannelReady') && local >= minLocalCkb;
		})) {
			return true;
		}
		await new Promise((resolve) => setTimeout(resolve, 4000));
	}
	return false;
}

export async function openChannel(
	peerId: string,
	fundingCkb: number,
	isPublic = true,
): Promise<{ temporary_channel_id: string }> {
	const fundingAmount = ckbToShannons(fundingCkb);
	return client.call('open_channel', [{
		peer_id: peerId,
		funding_amount: fundingAmount,
		public: isPublic,
		funding_udt_type_script: null,
		shutdown_script: null,
		tlc_expiry_delta: toHexUint(14_400_000),
		tlc_min_value: toHexUint(0),
		tlc_fee_proportional_millionths: toHexUint(1000),
		max_tlc_number_in_flight: toHexUint(125),
	}]);
}

export async function listChannels(peerId?: string): Promise<{ channels: FiberChannel[] }> {
	return client.call('list_channels', [{
		peer_id: peerId ?? null,
		include_closed: false,
	}]);
}

export async function shutdownChannel(
	channelId: string,
	force = false,
): Promise<void> {
	await client.call('shutdown_channel', [{
		channel_id: channelId,
		close_script: null,
		fee_rate: 1000,
		force,
	}]);
}

export async function newInvoice(
	amountCkb: number,
	description: string,
	expirySeconds = 3600,
): Promise<FiberInvoice> {
	const currency = process.env.FIBER_CURRENCY ?? 'Fibt';
	return client.call('new_invoice', [{
		amount: ckbToShannons(amountCkb),
		description,
		currency,
		payment_preimage: null,
		payment_hash: null,
		expiry: expirySeconds,
		final_expiry_delta: 86_400_000,
	}]);
}

/// Creates a hold invoice with a pre-determined payment_hash.
/// The invoice cannot be settled until the preimage is revealed via settleInvoice().
export async function newHoldInvoice(
	amountCkb: number,
	paymentHash: string,
	description: string,
	expirySeconds = 3600,
): Promise<FiberHoldInvoice> {
	const currency = process.env.FIBER_CURRENCY ?? 'Fibt';
	return client.call('new_invoice', [{
		amount: ckbToShannons(amountCkb),
		description,
		currency,
		payment_preimage: null,
		payment_hash: paymentHash,
		expiry: expirySeconds,
		final_expiry_delta: 86_400_000,
	}]);
}

/// Settles a hold invoice by revealing the preimage for the given payment_hash.
export async function settleInvoice(
	paymentHash: string,
	preimage: string,
): Promise<void> {
	await client.call('settle_invoice', [{
		payment_hash: paymentHash,
		payment_preimage: preimage,
	}]);
}

/// Gets the status of an invoice by payment_hash.
export async function getInvoice(
	paymentHash: string,
): Promise<FiberHoldInvoice> {
	return client.call('get_invoice', [{
		payment_hash: paymentHash,
	}]);
}

// Send payment by invoice string (preferred) or keysend (target_pubkey + amount).
export async function sendPayment(opts: {
	invoice?: string;
	targetPubkey?: string;
	amountCkb?: number;
	description?: string;
}): Promise<FiberPaymentResult> {
	const params: Record<string, unknown> = {
		timeout: toHexUint(300),
		max_fee_amount: toHexUint(ckbToShannons(0.01)),
		max_parts: toHexUint(1),
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
		params.amount = toHexUint(ckbToShannons(opts.amountCkb));
	}

	return client.call('send_payment', [params]);
}

/// Check if the Fiber node is reachable and ready to process payments.
export async function isNodeReady(): Promise<boolean> {
	try {
		await client.call('node_info', [{}]);
		return true;
	} catch {
		return false;
	}
}
