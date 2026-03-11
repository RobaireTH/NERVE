import express from 'express';

const app = express();
const PORT = Number(process.env.MCP_PORT ?? 8081);

app.use(express.json());

app.get('/health', (_req, res) => {
	res.json({ status: 'ok', service: 'nerve-mcp' });
});

app.listen(PORT, () => {
	console.log(`nerve-mcp bridge listening on :${PORT}`);
});
