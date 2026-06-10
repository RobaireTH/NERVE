import { describe, expect, it } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { PayrollStore } from './store';

describe('PayrollStore', () => {
	it('persists contractors and payouts to disk', async () => {
		const dir = mkdtempSync(path.join(tmpdir(), 'fiber-payroll-store-'));
		const store = new PayrollStore(dir);

		const contractor = await store.createContractor({
			name: 'Amina Yusuf',
			email: 'amina@example.com',
			notes: 'QA contractor',
		});

		await store.createPayout({
			contractorId: contractor.id,
			amountCkb: 120,
			description: 'May review payout',
		});

		const reloaded = new PayrollStore(dir);
		const contractors = await reloaded.listContractors();
		const payouts = await reloaded.listPayouts();

		expect(contractors).toHaveLength(1);
		expect(contractors[0]?.portalToken).toBeTruthy();
		expect(payouts).toHaveLength(1);
		expect(payouts[0]?.status).toBe('draft');
	});
});
