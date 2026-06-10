import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { PortalPage } from './PortalPage';

afterEach(() => {
	cleanup();
});

describe('PortalPage', () => {
	it('shows payout details and allows profile updates', async () => {
		const api = {
			getPortal: vi.fn().mockResolvedValue({
				contractor: {
					name: 'Amina Yusuf',
					payoutAddress: 'fibt_address_1',
					lockArgs: '0x1111111111111111111111111111111111111111',
					fiberNodeId: 'node-1',
					fiberRpcUrl: 'http://worker-fiber:8227',
				},
				payouts: [
					{
						id: 'payout-1',
						amountCkb: 120,
						status: 'paid',
						fiberSettlementStatus: 'paid',
					},
				],
			}),
			updatePortalProfile: vi.fn().mockResolvedValue({
				contractor: {
					name: 'Amina Yusuf',
					payoutAddress: 'fibt_address_2',
					lockArgs: '0x1111111111111111111111111111111111111111',
				},
			}),
		};

		render(<PortalPage token="token-1" api={api as never} />);

		expect(await screen.findByText('Amina Yusuf')).toBeInTheDocument();
		expect(screen.getByText('120 CKB')).toBeInTheDocument();
		expect(
			screen.getByRole('button', { name: /save payout details/i }),
		).toBeInTheDocument();
	});

	it('submits updated payout details', async () => {
		const api = {
			getPortal: vi.fn().mockResolvedValue({
				contractor: {
					name: 'Amina Yusuf',
					payoutAddress: 'fibt_address_1',
					lockArgs: '0x1111111111111111111111111111111111111111',
					fiberNodeId: 'node-1',
					fiberRpcUrl: 'http://worker-fiber:8227',
				},
				payouts: [],
			}),
			updatePortalProfile: vi.fn().mockResolvedValue({
				contractor: {
					name: 'Amina Yusuf',
					payoutAddress: 'fibt_address_2',
					lockArgs: '0x2222222222222222222222222222222222222222',
					fiberNodeId: 'node-2',
					fiberRpcUrl: 'http://worker-fiber:9227',
				},
			}),
		};

		render(<PortalPage token="token-1" api={api as never} />);

		await screen.findByText('Amina Yusuf');

		fireEvent.change(screen.getByLabelText('Payout address'), {
			target: { value: 'fibt_address_2' },
		});
		fireEvent.change(screen.getByLabelText('Lock args'), {
			target: { value: '0x2222222222222222222222222222222222222222' },
		});
		fireEvent.change(screen.getByLabelText('Fiber node'), {
			target: { value: 'node-2' },
		});
		fireEvent.click(screen.getByRole('button', { name: /save payout details/i }));

		await waitFor(() => {
			expect(api.updatePortalProfile).toHaveBeenCalledWith('token-1', {
				payoutAddress: 'fibt_address_2',
				lockArgs: '0x2222222222222222222222222222222222222222',
				fiberNodeId: 'node-2',
				fiberRpcUrl: 'http://worker-fiber:8227',
			});
		});
	});
});
