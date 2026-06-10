import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { ContractorsPage } from './ContractorsPage';

afterEach(() => {
	cleanup();
});

describe('ContractorsPage', () => {
	it('shows contractors and a create payout action', async () => {
		const api = {
			listContractors: vi.fn().mockResolvedValue([
				{
					id: '1',
					name: 'Amina Yusuf',
					email: 'amina@example.com',
					notes: 'QA',
					portalToken: 'token-1',
					lockArgs: '0x1111111111111111111111111111111111111111',
					fiberNodeId: 'node-1',
					payoutAddress: 'fibt_address_1',
				},
			]),
			listLedger: vi.fn().mockResolvedValue([]),
		};

		render(<ContractorsPage api={api as never} />);

		expect(await screen.findByText('Amina Yusuf')).toBeInTheDocument();
		expect(screen.getAllByRole('button', { name: /create payout/i })).toHaveLength(2);
		expect(screen.getByText('Ready')).toBeInTheDocument();
		expect(screen.getByRole('link', { name: /open portal/i })).toHaveAttribute(
			'href',
			'/portal/token-1',
		);
	});

	it('creates a contractor and executes a payout from the dashboard flow', async () => {
		const api = {
			listContractors: vi.fn().mockResolvedValue([]),
			listLedger: vi.fn().mockResolvedValue([]),
			createContractor: vi.fn().mockResolvedValue({
				id: 'contractor-1',
				name: 'Amina Yusuf',
				email: 'amina@example.com',
				notes: 'QA contractor',
				portalToken: 'token-1',
			}),
			createPayout: vi.fn().mockResolvedValue({
				id: 'payout-1',
				contractorId: 'contractor-1',
				amountCkb: 120,
				description: 'May review payout',
			}),
			executePayout: vi.fn().mockResolvedValue({
				id: 'payout-1',
				status: 'paid',
				fiberSettlementStatus: 'paid',
			}),
		};

		render(<ContractorsPage api={api as never} />);

		const forms = screen.getAllByText('Contractor name');
		expect(forms).toHaveLength(1);

		fireEvent.change(screen.getByLabelText('Contractor name'), {
			target: { value: 'Amina Yusuf' },
		});
		fireEvent.change(screen.getByLabelText('Email'), {
			target: { value: 'amina@example.com' },
		});
		fireEvent.change(screen.getByLabelText('Notes'), {
			target: { value: 'QA contractor' },
		});
		fireEvent.click(screen.getByRole('button', { name: /add contractor/i }));

		await screen.findByText('Amina Yusuf');

		fireEvent.change(screen.getByLabelText('Payout amount (CKB)'), {
			target: { value: '120' },
		});
		fireEvent.change(screen.getByLabelText('Payout description'), {
			target: { value: 'May review payout' },
		});
		fireEvent.click(screen.getAllByRole('button', { name: /create payout/i })[1]!);

		await waitFor(() => {
			expect(api.createPayout).toHaveBeenCalledWith({
				contractorId: 'contractor-1',
				amountCkb: 120,
				description: 'May review payout',
			});
		});

		fireEvent.click(screen.getByRole('button', { name: /execute latest payout/i }));

		await waitFor(() => {
			expect(api.executePayout).toHaveBeenCalledWith('payout-1');
		});

		expect(await screen.findByText('Payout sent: paid / paid')).toBeInTheDocument();
	});
});
