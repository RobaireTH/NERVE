import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import type { ContractorRecord, PayoutRecord } from './types';

interface PayrollState {
	contractors: ContractorRecord[];
	payouts: PayoutRecord[];
}

type CreateContractorInput = Pick<ContractorRecord, 'name' | 'email' | 'notes'>;
type CreatePayoutInput = Pick<PayoutRecord, 'contractorId' | 'amountCkb' | 'description'>;

export class PayrollStore {
	constructor(private readonly dataDir: string) {}

	private async readState(): Promise<PayrollState> {
		const file = path.join(this.dataDir, 'payroll.json');
		try {
			const raw = await fs.readFile(file, 'utf8');
			return JSON.parse(raw) as PayrollState;
		} catch {
			return { contractors: [], payouts: [] };
		}
	}

	private async writeState(state: PayrollState): Promise<void> {
		await fs.mkdir(this.dataDir, { recursive: true });
		await fs.writeFile(
			path.join(this.dataDir, 'payroll.json'),
			JSON.stringify(state, null, 2) + '\n',
		);
	}

	async listContractors(): Promise<ContractorRecord[]> {
		return (await this.readState()).contractors;
	}

	async listPayouts(): Promise<PayoutRecord[]> {
		return (await this.readState()).payouts;
	}

	async createContractor(input: CreateContractorInput): Promise<ContractorRecord> {
		const state = await this.readState();
		const now = new Date().toISOString();
		const contractor: ContractorRecord = {
			id: crypto.randomUUID(),
			name: input.name,
			email: input.email,
			lockArgs: null,
			fiberNodeId: null,
			fiberRpcUrl: null,
			payoutAddress: null,
			notes: input.notes,
			portalToken: crypto.randomUUID(),
			createdAt: now,
			updatedAt: now,
		};
		state.contractors.push(contractor);
		await this.writeState(state);
		return contractor;
	}

	async createPayout(input: CreatePayoutInput): Promise<PayoutRecord> {
		const state = await this.readState();
		const now = new Date().toISOString();
		const payout: PayoutRecord = {
			id: crypto.randomUUID(),
			contractorId: input.contractorId,
			amountCkb: input.amountCkb,
			description: input.description,
			status: 'draft',
			jobTxHash: null,
			jobIndex: null,
			reserveTxHash: null,
			claimTxHash: null,
			completeTxHash: null,
			fiberMode: 'direct',
			fiberPaymentHash: null,
			fiberSettlementStatus: 'not_requested',
			failureReason: null,
			createdAt: now,
			updatedAt: now,
		};
		state.payouts.push(payout);
		await this.writeState(state);
		return payout;
	}

	async updateContractor(
		id: string,
		patch: Partial<Pick<ContractorRecord, 'lockArgs' | 'fiberNodeId' | 'fiberRpcUrl' | 'payoutAddress' | 'notes' | 'name' | 'email'>>,
	): Promise<ContractorRecord | null> {
		const state = await this.readState();
		const contractor = state.contractors.find((item) => item.id === id);
		if (!contractor) return null;

		Object.assign(contractor, patch, {
			updatedAt: new Date().toISOString(),
		});
		await this.writeState(state);
		return contractor;
	}

	async getContractor(id: string): Promise<ContractorRecord | null> {
		return (await this.readState()).contractors.find((item) => item.id === id) ?? null;
	}

	async getPayout(id: string): Promise<PayoutRecord | null> {
		return (await this.readState()).payouts.find((item) => item.id === id) ?? null;
	}

	async findContractorByPortalToken(token: string): Promise<ContractorRecord | null> {
		return (await this.readState()).contractors.find((item) => item.portalToken === token) ?? null;
	}

	async updateContractorByPortalToken(
		token: string,
		patch: Partial<Pick<ContractorRecord, 'lockArgs' | 'fiberNodeId' | 'fiberRpcUrl' | 'payoutAddress' | 'notes' | 'name' | 'email'>>,
	): Promise<ContractorRecord | null> {
		const contractor = await this.findContractorByPortalToken(token);
		if (!contractor) return null;
		return this.updateContractor(contractor.id, patch);
	}

	async listPayoutsForContractor(contractorId: string): Promise<PayoutRecord[]> {
		return (await this.readState()).payouts.filter((item) => item.contractorId === contractorId);
	}

	async savePayout(next: PayoutRecord): Promise<void> {
		const state = await this.readState();
		const index = state.payouts.findIndex((item) => item.id === next.id);
		if (index === -1) {
			state.payouts.push(next);
		} else {
			state.payouts[index] = next;
		}
		await this.writeState(state);
	}
}
