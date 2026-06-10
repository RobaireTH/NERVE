import { Router } from 'express';
import { PayrollStore } from '../payroll/store.js';

const router = Router();

function getStore() {
	return new PayrollStore(process.env.PAYROLL_DATA_DIR ?? '/app/runtime-data');
}

router.get('/:token', async (req, res) => {
	const contractor = await getStore().findContractorByPortalToken(req.params.token);
	if (!contractor) {
		res.status(404).json({ error: 'portal token not found' });
		return;
	}
	res.json({ contractor });
});

router.patch('/:token/profile', async (req, res) => {
	const contractor = await getStore().updateContractorByPortalToken(req.params.token, req.body);
	if (!contractor) {
		res.status(404).json({ error: 'portal token not found' });
		return;
	}
	res.json({ contractor });
});

router.get('/:token/payouts', async (req, res) => {
	const contractor = await getStore().findContractorByPortalToken(req.params.token);
	if (!contractor) {
		res.status(404).json({ error: 'portal token not found' });
		return;
	}
	const payouts = await getStore().listPayoutsForContractor(contractor.id);
	res.json({ contractor, payouts });
});

export default router;
