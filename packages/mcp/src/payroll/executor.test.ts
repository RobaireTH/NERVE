import { describe, expect, it, vi } from 'vitest';
import { executePayout } from './executor';
import type { ContractorRecord, PayoutRecord } from './types';

describe('executePayout', () => {
	it('drives post -> reserve -> claim -> complete and returns a paid ledger entry', async () => {
		const contractor = {
			id: 'contractor-1',
			name: 'Amina Yusuf',
			email: 'amina@example.com',
			lockArgs: '0x1111111111111111111111111111111111111111',
			fiberNodeId: 'node-1',
			fiberRpcUrl: 'http://worker-fiber:8227',
			payoutAddress: 'fibt_address_1',
			notes: '',
			portalToken: 'token-1',
			createdAt: '2026-06-09T00:00:00.000Z',
			updatedAt: '2026-06-09T00:00:00.000Z',
		} satisfies ContractorRecord;

		const payout = {
			id: 'payout-1',
			contractorId: contractor.id,
			amountCkb: 120,
			description: 'May review payout',
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
			createdAt: '2026-06-09T00:00:00.000Z',
			updatedAt: '2026-06-09T00:00:00.000Z',
		} satisfies PayoutRecord;

		const coreFetch = vi.fn()
			.mockResolvedValueOnce({ ok: true, json: async () => ({ tx_hash: '0xpost' }) })
			.mockResolvedValueOnce({ ok: true, json: async () => ({ tx_hash: '0xreserve' }) })
			.mockResolvedValueOnce({ ok: true, json: async () => ({ tx_hash: '0xclaim' }) })
			.mockResolvedValueOnce({
				ok: true,
				json: async () => ({
					tx_hash: '0xcomplete',
					fiber_payment_requested: true,
					fiber_payment_mode: 'direct',
				}),
			});

		const updated = await executePayout({
			payout,
			contractor,
			coreFetch: coreFetch as unknown as typeof fetch,
			now: () => '2026-06-09T12:00:00.000Z',
		});

		expect(updated.status).toBe('paid');
		expect(updated.jobTxHash).toBe('0xpost');
		expect(updated.reserveTxHash).toBe('0xreserve');
		expect(updated.claimTxHash).toBe('0xclaim');
		expect(updated.completeTxHash).toBe('0xcomplete');
		expect(updated.fiberSettlementStatus).toBe('paid');
	});
});
