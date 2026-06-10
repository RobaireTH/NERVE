import { afterEach, describe, expect, it } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createApp } from '../app';
import { invokeApp } from '../test/invoke-app';

function makeDataDir(name: string) {
	return fs.mkdtempSync(path.join(os.tmpdir(), name));
}

describe('contractor portal API', () => {
	let dataDir = '';

	afterEach(() => {
		if (dataDir) {
			fs.rmSync(dataDir, { recursive: true, force: true });
			dataDir = '';
		}
		delete process.env.PAYROLL_DATA_DIR;
	});

	it('updates payout profile through a portal token', async () => {
		dataDir = makeDataDir('fiber-payroll-portal-');
		process.env.PAYROLL_DATA_DIR = dataDir;
		const app = createApp();

		const created = await invokeApp(app, {
			method: 'POST',
			url: '/payroll/contractors',
			body: { name: 'Amina Yusuf', email: 'amina@example.com', notes: 'QA contractor' },
		});

		const token = (created.body as { contractor: { portalToken: string } }).contractor.portalToken;

		const update = await invokeApp(app, {
			method: 'PATCH',
			url: `/portal/${token}/profile`,
			body: {
				lockArgs: '0x1111111111111111111111111111111111111111',
				fiberNodeId: 'node-1',
				fiberRpcUrl: 'http://worker-fiber:8227',
				payoutAddress: 'fibt_address_1',
			},
		});

		expect(update.status).toBe(200);
		expect((update.body as { contractor: { lockArgs: string } }).contractor.lockArgs).toBe(
			'0x1111111111111111111111111111111111111111',
		);
	});
});
