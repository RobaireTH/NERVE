export type PayoutStatus =
	| 'draft'
	| 'pending_contractor_details'
	| 'queued'
	| 'processing'
	| 'paid'
	| 'payment_failed';

export type FiberSettlementStatus =
	| 'not_requested'
	| 'pending'
	| 'paid'
	| 'failed';

export interface ContractorRecord {
	id: string;
	name: string;
	email: string;
	lockArgs: string | null;
	fiberNodeId: string | null;
	fiberRpcUrl: string | null;
	payoutAddress: string | null;
	notes: string;
	portalToken: string;
	createdAt: string;
	updatedAt: string;
}

export interface PayoutRecord {
	id: string;
	contractorId: string;
	amountCkb: number;
	description: string;
	status: PayoutStatus;
	jobTxHash: string | null;
	jobIndex: number | null;
	reserveTxHash: string | null;
	claimTxHash: string | null;
	completeTxHash: string | null;
	fiberMode: 'direct' | 'hold';
	fiberPaymentHash: string | null;
	fiberSettlementStatus: FiberSettlementStatus;
	failureReason: string | null;
	createdAt: string;
	updatedAt: string;
}

