import type { ContractorRecord, PayoutRecord } from './types';

const ZERO_CAPABILITY_HASH = `0x${'0'.repeat(64)}`;

interface ExecutePayoutArgs {
	payout: PayoutRecord;
	contractor: ContractorRecord;
	coreFetch?: typeof fetch;
	now?: () => string;
}

async function callCore(
	coreFetch: typeof fetch,
	body: Record<string, unknown>,
): Promise<Record<string, unknown>> {
	const response = await coreFetch(
		`${process.env.CORE_URL ?? 'http://localhost:8080'}/tx/build-and-broadcast`,
		{
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(body),
		},
	);
	const data = (await response.json()) as Record<string, unknown>;
	if (!response.ok) {
		throw new Error(String(data.error ?? 'core request failed'));
	}
	return data;
}

export async function executePayout({
	payout,
	contractor,
	coreFetch = fetch,
	now = () => new Date().toISOString(),
}: ExecutePayoutArgs): Promise<PayoutRecord> {
	if (!contractor.lockArgs || !contractor.fiberNodeId || !contractor.fiberRpcUrl) {
		return {
			...payout,
			status: 'pending_contractor_details',
			failureReason: 'contractor payout details are incomplete',
			updatedAt: now(),
		};
	}

	const payment = {
		mode: 'fiber',
		invoice_mode: payout.fiberMode,
		lock_args: contractor.lockArgs,
		node_id: contractor.fiberNodeId,
		rpc_url: contractor.fiberRpcUrl,
		amount_ckb: payout.amountCkb,
		description: payout.description,
	};

	const posted = await callCore(coreFetch, {
		intent: 'post_job',
		reward_ckb: payout.amountCkb,
		ttl_blocks: 200,
		capability_hash: ZERO_CAPABILITY_HASH,
		description: `Payroll payout for ${contractor.name}\n\nNERVE_PAYMENT:${JSON.stringify(payment)}`,
	});

	const jobTxHash = String(posted.tx_hash);
	const reserve = await callCore(coreFetch, {
		intent: 'reserve_job',
		job_tx_hash: jobTxHash,
		job_index: 0,
		worker_lock_args: contractor.lockArgs,
	});
	const claim = await callCore(coreFetch, {
		intent: 'claim_job',
		job_tx_hash: jobTxHash,
		job_index: 0,
	});
	const complete = await callCore(coreFetch, {
		intent: 'complete_job',
		job_tx_hash: jobTxHash,
		job_index: 0,
		worker_lock_args: contractor.lockArgs,
		result: `Payroll settled for ${contractor.name} (${payout.amountCkb} CKB)`,
	});

	return {
		...payout,
		status: 'paid',
		jobTxHash,
		jobIndex: 0,
		reserveTxHash: String(reserve.tx_hash),
		claimTxHash: String(claim.tx_hash),
		completeTxHash: String(complete.tx_hash),
		fiberSettlementStatus: 'paid',
		failureReason: null,
		updatedAt: now(),
	};
}
