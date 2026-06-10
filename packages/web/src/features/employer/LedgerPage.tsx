import { useEffect, useState } from 'react';
import { LedgerRow, payrollApi } from '../../lib/api';

export function LedgerPage({
	api = payrollApi,
}: {
	api?: typeof payrollApi;
}) {
	const [rows, setRows] = useState<LedgerRow[]>([]);

	useEffect(() => {
		void api.listLedger().then(setRows);
	}, [api]);

	return (
		<section className="stack-lg">
			<header className="hero-panel">
				<div>
					<h1>Payroll Ledger</h1>
					<p>Track payout execution across job lifecycle and Fiber settlement.</p>
				</div>
			</header>

			<div className="data-panel">
				<table className="ledger-table">
					<thead>
						<tr>
							<th>Payout</th>
							<th>Amount</th>
							<th>Status</th>
							<th>Fiber</th>
							<th>Updated</th>
						</tr>
					</thead>
					<tbody>
						{rows.map((row) => (
							<tr key={row.id}>
								<td>{row.id}</td>
								<td>{row.amountCkb} CKB</td>
								<td>
									<div className="ledger-status">
										<strong>{row.status}</strong>
										{row.failureReason ? (
											<span className="ledger-subtext">{row.failureReason}</span>
										) : null}
									</div>
								</td>
								<td>{row.fiberSettlementStatus}</td>
								<td>{new Date(row.updatedAt).toLocaleString()}</td>
							</tr>
						))}
					</tbody>
				</table>
			</div>
		</section>
	);
}
