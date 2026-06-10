import { afterEach, describe, expect, it } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { createApp } from '../app';
import { invokeApp } from '../test/invoke-app';

function makeDataDir(name: string) {
	return fs.mkdtempSync(path.join(os.tmpdir(), name));
}

describe('payroll contractors API', () => {
	let dataDir = '';

	afterEach(() => {
		if (dataDir) {
			fs.rmSync(dataDir, { recursive: true, force: true });
			dataDir = '';
		}
		delete process.env.PAYROLL_DATA_DIR;
	});

	it('creates and lists contractors', async () => {
		dataDir = makeDataDir('fiber-payroll-contractors-');
		process.env.PAYROLL_DATA_DIR = dataDir;
		const app = createApp();

		const create = await invokeApp(app, {
			method: 'POST',
			url: '/payroll/contractors',
			body: { name: 'Amina Yusuf', email: 'amina@example.com', notes: 'QA contractor' },
		});

		expect(create.status).toBe(201);
		expect((create.body as { contractor: { name: string } }).contractor.name).toBe('Amina Yusuf');

		const list = await invokeApp(app, {
			method: 'GET',
			url: '/payroll/contractors',
		});
		expect(list.status).toBe(200);
		expect((list.body as { contractors: unknown[] }).contractors).toHaveLength(1);
	});
});
