import { createApp } from './app.js';
const PORT = Number(process.env.MCP_PORT ?? 8081);
createApp().listen(PORT, () => {
	console.log(`nerve-mcp bridge listening on :${PORT}`);
});
