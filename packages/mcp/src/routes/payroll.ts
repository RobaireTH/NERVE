import { Router } from 'express';
import { executePayout } from '../payroll/executor.js';
import { PayrollStore } from '../payroll/store.js';

const router = Router();

function getStore() {
	return new PayrollStore(process.env.PAYROLL_DATA_DIR ?? '/app/runtime-data');
}

router.get('/contractors', async (_req, res) => {
	const contractors = await getStore().listContractors();
	res.json({ contractors });
});

router.post('/contractors', async (req, res) => {
	const { name, email, notes = '' } = req.body as {
		name?: string;
		email?: string;
		notes?: string;
	};
	if (!name || !email) {
		res.status(400).json({ error: 'name and email are required' });
		return;
	}

	const contractor = await getStore().createContractor({ name, email, notes });
	res.status(201).json({ contractor });
});

router.patch('/contractors/:id', async (req, res) => {
	const contractor = await getStore().updateContractor(req.params.id, req.body);
	if (!contractor) {
		res.status(404).json({ error: 'contractor not found' });
		return;
	}
	res.json({ contractor });
});

router.get('/payouts', async (_req, res) => {
	const payouts = await getStore().listPayouts();
	res.json({ payouts });
});

router.post('/payouts', async (req, res) => {
	const { contractorId, amountCkb, description } = req.body as {
		contractorId?: string;
		amountCkb?: number;
		description?: string;
	};
	if (!contractorId || amountCkb === undefined || !description) {
		res.status(400).json({ error: 'contractorId, amountCkb, and description are required' });
		return;
	}
	const payout = await getStore().createPayout({ contractorId, amountCkb, description });
	res.status(201).json({ payout });
});

router.get('/ledger', async (_req, res) => {
	const payouts = await getStore().listPayouts();
	res.json({
		ledger: payouts.map((payout) => ({
			id: payout.id,
			contractorId: payout.contractorId,
			amountCkb: payout.amountCkb,
			status: payout.status,
			fiberSettlementStatus: payout.fiberSettlementStatus,
			jobTxHash: payout.jobTxHash,
			completeTxHash: payout.completeTxHash,
			failureReason: payout.failureReason,
			updatedAt: payout.updatedAt,
		})),
	});
});

router.post('/payouts/:id/execute', async (req, res) => {
	const store = getStore();
	const payout = await store.getPayout(req.params.id);
	if (!payout) {
		res.status(404).json({ error: 'payout not found' });
		return;
	}

	const contractor = await store.getContractor(payout.contractorId);
	if (!contractor) {
		res.status(404).json({ error: 'contractor not found' });
		return;
	}

	const updated = await executePayout({ payout, contractor });
	await store.savePayout(updated);
	res.json({ payout: updated });
});

export default router;
