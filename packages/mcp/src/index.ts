import express from 'express';
import chainRouter from './routes/chain.js';
import jobsRouter from './routes/jobs.js';

const app = express();
const PORT = Number(process.env.MCP_PORT ?? 8081);

app.use(express.json());

app.get('/health', (_req, res) => {
	res.json({ status: 'ok', service: 'nerve-mcp' });
});

app.use('/chain', chainRouter);
app.use('/jobs', jobsRouter);

app.listen(PORT, () => {
	console.log(`nerve-mcp bridge listening on :${PORT}`);
});
