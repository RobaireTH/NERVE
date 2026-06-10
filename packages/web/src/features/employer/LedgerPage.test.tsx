import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import { LedgerPage } from './LedgerPage';

afterEach(() => {
	cleanup();
});

describe('LedgerPage', () => {
	it('renders payout rows with status and failure details', async () => {
		const api = {
			listLedger: vi.fn().mockResolvedValue([
				{
					id: 'payout-1',
					amountCkb: 120,
					status: 'paid',
					fiberSettlementStatus: 'paid',
					failureReason: null,
					updatedAt: '2026-06-10T00:00:00.000Z',
				},
				{
					id: 'payout-2',
					amountCkb: 45,
					status: 'payment_failed',
					fiberSettlementStatus: 'failed',
					failureReason: 'missing contractor payout details',
					updatedAt: '2026-06-10T00:00:00.000Z',
				},
			]),
		};

		render(<LedgerPage api={api as never} />);

		expect(await screen.findByText('payout-1')).toBeInTheDocument();
		const failedRow = screen.getByText('payout-2').closest('tr');
		expect(failedRow).not.toBeNull();
		expect(within(failedRow!).getByText('payment_failed')).toBeInTheDocument();
		expect(within(failedRow!).getByText('failed')).toBeInTheDocument();
		expect(screen.getByText('payment_failed')).toBeInTheDocument();
		expect(screen.getByText('missing contractor payout details')).toBeInTheDocument();
	});
});
