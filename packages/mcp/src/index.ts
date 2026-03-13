import express, { Request, Response, NextFunction } from 'express';
import chainRouter from './routes/chain.js';
import jobsRouter from './routes/jobs.js';
import agentsRouter from './routes/agents.js';
import fiberRouter from './routes/fiber.js';

const app = express();
const PORT = Number(process.env.MCP_PORT ?? 8081);

app.use(express.json({ limit: '1mb' }));

app.get('/health', (_req, res) => {
	res.json({ status: 'ok', service: 'nerve-mcp' });
});

app.use('/chain', chainRouter);
app.use('/jobs', jobsRouter);
app.use('/agents', agentsRouter);
app.use('/fiber', fiberRouter);

// Global error handler — catches unhandled exceptions in route handlers.
app.use((err: Error, _req: Request, res: Response, _next: NextFunction) => {
	console.error('unhandled error:', err.message);
	res.status(500).json({ error: 'internal_server_error' });
});

app.listen(PORT, () => {
	console.log(`nerve-mcp bridge listening on :${PORT}`);
});
