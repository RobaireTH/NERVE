import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { App } from './App';

afterEach(() => {
	cleanup();
	vi.unstubAllGlobals();
});

describe('App shell', () => {
	it('renders the Fiber Payroll brand and dashboard nav', () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				json: async () => ({ contractors: [], ledger: [] }),
			}),
		);

		render(<App initialPath="/" />);

		expect(screen.getByText('Fiber Payroll')).toBeInTheDocument();
	expect(screen.getByRole('navigation', { name: 'Primary' })).toBeInTheDocument();
	expect(screen.getByRole('link', { name: 'Contractors' })).toBeInTheDocument();
	expect(screen.getByRole('link', { name: 'Ledger' })).toBeInTheDocument();
	});

	it('switches to the ledger view when the ledger nav link is clicked', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn().mockResolvedValue({
				json: async () => ({ contractors: [], ledger: [] }),
			}),
		);

		render(<App initialPath="/" />);
		fireEvent.click(screen.getAllByRole('link', { name: 'Ledger' })[0]!);

		expect(await screen.findByText('Payroll Ledger')).toBeInTheDocument();
	});
});
