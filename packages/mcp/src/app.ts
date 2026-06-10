import express, { Request, Response, NextFunction } from 'express';
import path from 'path';
import chainRouter from './routes/chain.js';
import jobsRouter from './routes/jobs.js';
import agentsRouter from './routes/agents.js';
import fiberRouter from './routes/fiber.js';
import discoverRouter from './routes/discover.js';
import txRouter from './routes/tx.js';
import payrollRouter from './routes/payroll.js';
import portalRouter from './routes/portal.js';

export function createApp() {
	const app = express();

	app.use(express.json({ limit: '1mb' }));
	app.use('/docs', express.static(path.resolve(__dirname, '../docs')));

	app.get('/health', (_req, res) => {
		res.json({ status: 'ok', service: 'nerve-mcp' });
	});

	app.use('/', discoverRouter);
	app.use('/chain', chainRouter);
	app.use('/jobs', jobsRouter);
	app.use('/agents', agentsRouter);
	app.use('/fiber', fiberRouter);
	app.use('/tx', txRouter);
	app.use('/payroll', payrollRouter);
	app.use('/portal', portalRouter);

	app.use((err: Error, _req: Request, res: Response, _next: NextFunction) => {
		console.error('unhandled error:', err.message);
		res.status(500).json({ error: 'internal_server_error' });
	});

	return app;
}
