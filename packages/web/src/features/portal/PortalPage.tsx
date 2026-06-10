import { useEffect, useState } from 'react';
import { payrollApi } from '../../lib/api';

interface PortalContractor {
	name: string;
	lockArgs: string | null;
	fiberNodeId: string | null;
	fiberRpcUrl: string | null;
	payoutAddress: string | null;
}

interface PortalPayout {
	id: string;
	amountCkb: number;
	status: string;
	fiberSettlementStatus: string;
}

export function PortalPage({
	token,
	api = payrollApi,
}: {
	token: string;
	api?: typeof payrollApi;
}) {
	const [profile, setProfile] = useState<PortalContractor>({
		name: '',
		lockArgs: '',
		fiberNodeId: '',
		fiberRpcUrl: '',
		payoutAddress: '',
	});
	const [payouts, setPayouts] = useState<PortalPayout[]>([]);
	const [statusMessage, setStatusMessage] = useState('');

	useEffect(() => {
		void api.getPortal(token).then(({ contractor, payouts }) => {
			setProfile({
				name: contractor.name,
				lockArgs: contractor.lockArgs ?? '',
				fiberNodeId: contractor.fiberNodeId ?? '',
				fiberRpcUrl: contractor.fiberRpcUrl ?? '',
				payoutAddress: contractor.payoutAddress ?? '',
			});
			setPayouts(payouts);
		});
	}, [api, token]);

	async function handleSave() {
		const result = await api.updatePortalProfile(token, {
			payoutAddress: profile.payoutAddress ?? '',
			lockArgs: profile.lockArgs ?? '',
			fiberNodeId: profile.fiberNodeId ?? '',
			fiberRpcUrl: profile.fiberRpcUrl ?? '',
		});
		const contractor = result.contractor as PortalContractor;
		setProfile({
			name: contractor.name,
			lockArgs: contractor.lockArgs ?? '',
			fiberNodeId: contractor.fiberNodeId ?? '',
			fiberRpcUrl: contractor.fiberRpcUrl ?? '',
			payoutAddress: contractor.payoutAddress ?? '',
		});
		setStatusMessage('Payout details saved');
	}

	return (
		<section className="stack-lg">
			<header className="hero-panel">
				<div>
					<h1>{profile.name || 'Contractor Portal'}</h1>
					<p>Update payout details and review payout status.</p>
				</div>
			</header>

			<form className="data-panel stack-md">
				<label className="field-stack">
					<span>Payout address</span>
					<input
						aria-label="Payout address"
						value={profile.payoutAddress ?? ''}
						onChange={(event) =>
							setProfile((current) => ({
								...current,
								payoutAddress: event.target.value,
							}))
						}
					/>
				</label>
				<label className="field-stack">
					<span>Lock args</span>
					<input
						aria-label="Lock args"
						value={profile.lockArgs ?? ''}
						onChange={(event) =>
							setProfile((current) => ({
								...current,
								lockArgs: event.target.value,
							}))
						}
					/>
				</label>
				<label className="field-stack">
					<span>Fiber node</span>
					<input
						aria-label="Fiber node"
						value={profile.fiberNodeId ?? ''}
						onChange={(event) =>
							setProfile((current) => ({
								...current,
								fiberNodeId: event.target.value,
							}))
						}
					/>
				</label>
				<label className="field-stack">
					<span>Fiber RPC URL</span>
					<input
						aria-label="Fiber RPC URL"
						value={profile.fiberRpcUrl ?? ''}
						onChange={(event) =>
							setProfile((current) => ({
								...current,
								fiberRpcUrl: event.target.value,
							}))
						}
					/>
				</label>
				<button className="primary-action" type="button" onClick={() => void handleSave()}>
					Save payout details
				</button>
				{statusMessage ? <p className="status-banner">{statusMessage}</p> : null}
			</form>

			<div className="data-panel">
				<h2>Recent payouts</h2>
				<ul className="timeline-list">
					{payouts.map((payout) => (
						<li key={payout.id}>
							<strong>{payout.amountCkb} CKB</strong>
							<span>{payout.status}</span>
							<span>{payout.fiberSettlementStatus}</span>
						</li>
					))}
				</ul>
			</div>
		</section>
	);
}

export function PortalRoute() {
	return <PortalPage token="" />;
}
