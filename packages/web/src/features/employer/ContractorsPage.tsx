import { useEffect, useState } from 'react';
import {
	ContractorSummary,
	payrollApi,
} from '../../lib/api';

export function ContractorsPage({
	api = payrollApi,
}: {
	api?: typeof payrollApi;
}) {
	const [contractors, setContractors] = useState<ContractorSummary[]>([]);
	const [form, setForm] = useState({
		name: '',
		email: '',
		notes: '',
		amountCkb: '120',
		description: '',
	});
	const [latestPayoutId, setLatestPayoutId] = useState<string | null>(null);
	const [statusMessage, setStatusMessage] = useState('');

	useEffect(() => {
		void api.listContractors().then(setContractors);
	}, [api]);

	async function handleCreateContractor() {
		if (!form.name || !form.email) return;
		const contractor = await api.createContractor({
			name: form.name,
			email: form.email,
			notes: form.notes,
		});
		setContractors((current) => [...current, contractor]);
		setStatusMessage(`Contractor added: ${contractor.name}`);
	}

	async function handleCreatePayout() {
		const contractor = contractors[contractors.length - 1];
		if (!contractor) return;
		const payout = await api.createPayout({
			contractorId: contractor.id,
			amountCkb: Number(form.amountCkb),
			description: form.description,
		});
		setLatestPayoutId(payout.id);
		setStatusMessage(`Payout created: ${payout.id}`);
	}

	async function handleExecutePayout() {
		if (!latestPayoutId) return;
		const payout = await api.executePayout(latestPayoutId);
		setStatusMessage(
			`Payout sent: ${payout.status} / ${payout.fiberSettlementStatus}`,
		);
	}

	return (
		<section className="stack-lg">
			<header className="hero-panel">
				<div>
					<h1>Contractors</h1>
					<p>Manage payout-ready contractors and send portal links.</p>
				</div>
				<button className="primary-action" type="button">
					Create payout
				</button>
			</header>

			<div className="data-panel stack-md">
				<div className="form-grid">
					<label className="field-stack">
						<span>Contractor name</span>
						<input
							aria-label="Contractor name"
							value={form.name}
							onChange={(event) =>
								setForm((current) => ({ ...current, name: event.target.value }))
							}
						/>
					</label>
					<label className="field-stack">
						<span>Email</span>
						<input
							aria-label="Email"
							value={form.email}
							onChange={(event) =>
								setForm((current) => ({ ...current, email: event.target.value }))
							}
						/>
					</label>
					<label className="field-stack field-span">
						<span>Notes</span>
						<input
							aria-label="Notes"
							value={form.notes}
							onChange={(event) =>
								setForm((current) => ({ ...current, notes: event.target.value }))
							}
						/>
					</label>
				</div>

				<div className="action-row">
					<button className="secondary-action" type="button" onClick={() => void handleCreateContractor()}>
						Add contractor
					</button>
				</div>

				<div className="form-grid">
					<label className="field-stack">
						<span>Payout amount (CKB)</span>
						<input
							aria-label="Payout amount (CKB)"
							value={form.amountCkb}
							onChange={(event) =>
								setForm((current) => ({ ...current, amountCkb: event.target.value }))
							}
						/>
					</label>
					<label className="field-stack field-span">
						<span>Payout description</span>
						<input
							aria-label="Payout description"
							value={form.description}
							onChange={(event) =>
								setForm((current) => ({ ...current, description: event.target.value }))
							}
						/>
					</label>
				</div>

				<div className="action-row">
					<button className="primary-action" type="button" onClick={() => void handleCreatePayout()}>
						Create payout
					</button>
					<button
						className="secondary-action"
						type="button"
						onClick={() => void handleExecutePayout()}
					>
						Execute latest payout
					</button>
				</div>

				{statusMessage ? <p className="status-banner">{statusMessage}</p> : null}
			</div>

			<div className="data-panel">
				<table className="ledger-table">
					<thead>
						<tr>
							<th>Name</th>
							<th>Email</th>
							<th>Status</th>
							<th>Notes</th>
							<th>Portal</th>
						</tr>
					</thead>
					<tbody>
						{contractors.map((contractor) => (
							<tr key={contractor.id}>
								<td>{contractor.name}</td>
								<td>{contractor.email}</td>
								<td>
									{contractor.lockArgs && contractor.fiberNodeId && contractor.payoutAddress
										? 'Ready'
										: 'Needs payout details'}
								</td>
								<td>{contractor.notes}</td>
								<td>
									<a
										className="inline-link"
										href={`/portal/${contractor.portalToken}`}
									>
										Open portal
									</a>
								</td>
							</tr>
						))}
					</tbody>
				</table>
			</div>
		</section>
	);
}
