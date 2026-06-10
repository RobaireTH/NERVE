const API_BASE = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:8081';

export interface ContractorSummary {
	id: string;
	name: string;
	email: string;
	notes: string;
	portalToken: string;
	lockArgs?: string | null;
	fiberNodeId?: string | null;
	payoutAddress?: string | null;
}

export interface LedgerRow {
	id: string;
	amountCkb: number;
	status: string;
	fiberSettlementStatus: string;
	failureReason?: string | null;
	updatedAt: string;
}

export const payrollApi = {
	async listContractors(): Promise<ContractorSummary[]> {
		const response = await fetch(`${API_BASE}/payroll/contractors`);
		return (await response.json()).contractors;
	},
	async createContractor(payload: {
		name: string;
		email: string;
		notes: string;
	}): Promise<ContractorSummary> {
		const response = await fetch(`${API_BASE}/payroll/contractors`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(payload),
		});
		return (await response.json()).contractor;
	},
	async listLedger(): Promise<LedgerRow[]> {
		const response = await fetch(`${API_BASE}/payroll/ledger`);
		return (await response.json()).ledger;
	},
	async createPayout(payload: {
		contractorId: string;
		amountCkb: number;
		description: string;
	}) {
		const response = await fetch(`${API_BASE}/payroll/payouts`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(payload),
		});
		return (await response.json()).payout;
	},
	async executePayout(payoutId: string) {
		const response = await fetch(`${API_BASE}/payroll/payouts/${payoutId}/execute`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
		});
		return (await response.json()).payout;
	},
	async getPortal(token: string) {
		const contractorResponse = await fetch(`${API_BASE}/portal/${token}`);
		const contractor = (await contractorResponse.json()).contractor;
		const payoutsResponse = await fetch(`${API_BASE}/portal/${token}/payouts`);
		const payouts = (await payoutsResponse.json()).payouts;
		return { contractor, payouts };
	},
	async updatePortalProfile(
		token: string,
		payload: {
			lockArgs: string;
			fiberNodeId: string;
			fiberRpcUrl: string;
			payoutAddress: string;
		},
	) {
		const response = await fetch(`${API_BASE}/portal/${token}/profile`, {
			method: 'PATCH',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(payload),
		});
		return await response.json();
	},
};
