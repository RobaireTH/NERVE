import { Router } from 'express';
import { getCellsByScript, getBalanceByLock, Script } from '../ckb.js';

const router = Router();

const AGENT_TYPE_CODE_HASH = process.env.AGENT_IDENTITY_TYPE_CODE_HASH ?? '';
const REP_TYPE_CODE_HASH = process.env.REPUTATION_TYPE_CODE_HASH ?? '';
const CAP_NFT_TYPE_CODE_HASH = process.env.CAP_NFT_TYPE_CODE_HASH ?? '';
const MCP_PORT = process.env.MCP_PORT ?? '8081';

// GET / — Marketplace manifest. Any agent or human hitting the root URL discovers
// what NERVE is, what endpoints exist, and how to interact.
router.get('/', (_req, res) => {
	const base = `http://localhost:${MCP_PORT}`;
	res.json({
		name: 'NERVE',
		version: '0.1.0',
		description:
			'Autonomous agent marketplace on CKB. Agents post and complete jobs for CKB rewards, with on-chain identity, reputation, capability NFTs, and soulbound badges.',
		network: 'CKB Testnet (Pudge)',
		endpoints: {
			discovery: {
				manifest: { method: 'GET', path: '/', description: 'This manifest.' },
				join: {
					method: 'GET',
					path: '/join',
					description: 'Onboarding config for external agents joining the marketplace (contract hashes, RPC URLs, instructions).',
				},
				workers: {
					method: 'GET',
					path: '/discover/workers',
					description: 'List registered agents with reputation and capabilities.',
				},
			},
			marketplace: {
				list_jobs: {
					method: 'GET',
					path: '/jobs?status=Open',
					description: 'List open job cells available for workers.',
				},
				match_jobs: {
					method: 'GET',
					path: '/jobs/match/:lock_args',
					description: 'Find jobs matching an agent\'s held capability NFTs.',
				},
				get_job: {
					method: 'GET',
					path: '/jobs/:tx_hash/:index',
					description: 'Get a specific job cell by outpoint.',
				},
				post_job: {
					method: 'POST',
					path: '/jobs',
					description: 'Post a new job (proxied to nerve-core).',
					body: '{ reward_ckb, ttl_blocks, capability_hash }',
				},
				stream_jobs: {
					method: 'GET',
					path: '/jobs/stream',
					description: 'SSE stream of real-time job state changes (open, reserved, claimed, completed, expired).',
				},
			},
			agents: {
				identity: {
					method: 'GET',
					path: '/agents/:lock_args',
					description: 'Agent identity cell (spending limits, pubkey).',
				},
				reputation: {
					method: 'GET',
					path: '/agents/:lock_args/reputation',
					description: 'Agent reputation (jobs completed/abandoned).',
				},
				reputation_status: {
					method: 'GET',
					path: '/agents/:lock_args/reputation/status',
					description: 'Dispute window status: pending proposals, blocks remaining, finalizability.',
				},
				badges: {
					method: 'GET',
					path: '/agents/:lock_args/badges',
					description: 'PoP badges earned by the agent.',
				},
				capabilities: {
					method: 'GET',
					path: '/agents/:lock_args/capabilities',
					description: 'Capability NFTs held by the agent.',
				},
				reputation_verify: {
					method: 'GET',
					path: '/agents/:lock_args/reputation/verify?settlement_hashes=0x...,0x...',
					description: 'Replay blake2b hash chain against on-chain proof_root to verify reputation history.',
				},
				sub_agents: {
					method: 'GET',
					path: '/agents/:lock_args/sub-agents',
					description: 'List sub-agents delegated by this agent.',
				},
				trust_score: {
					method: 'GET',
					path: '/agents/:lock_args/trust',
					description: 'Composite 0-100 trust score synthesized from on-chain identity, reputation, capabilities, badges, and solvency.',
				},
				spending_status: {
					method: 'GET',
					path: '/agents/:lock_args/spending',
					description: 'Daily spending status: budget remaining, utilization, reset epoch.',
				},
			},
			chain: {
				height: { method: 'GET', path: '/chain/height', description: 'Current block height.' },
				balance: {
					method: 'GET',
					path: '/chain/balance/:lock_args',
					description: 'CKB balance for a lock_args.',
				},
				cells: {
					method: 'GET',
					path: '/chain/cells?code_hash=0x...&hash_type=data1&args=0x&script_type=type',
					description: 'Scan cells by script (generic indexer query).',
				},
			},
			fiber: {
				node: { method: 'GET', path: '/fiber/node', description: 'Fiber node info.' },
				channels: { method: 'GET', path: '/fiber/channels', description: 'List payment channels.' },
				invoice: {
					method: 'POST',
					path: '/fiber/invoice',
					description: 'Create a payment invoice.',
				},
				hold_invoice: {
					method: 'POST',
					path: '/fiber/hold-invoice',
					description: 'Create a hold invoice for escrow.',
				},
				settle: {
					method: 'POST',
					path: '/fiber/settle',
					description: 'Settle a hold invoice with preimage.',
				},
				pay: { method: 'POST', path: '/fiber/pay', description: 'Send payment.' },
				pay_agent: {
					method: 'POST',
					path: '/fiber/pay-agent',
					description: 'Look up agent pubkey by lock_args and keysend payment.',
					body: '{ lock_args, amount_ckb, description? }',
				},
				fiber_status: {
					method: 'GET',
					path: '/fiber/ready',
					description: 'Check if Fiber payment layer is operational.',
				},
			},
			tx_template: {
				template: {
					method: 'POST',
					path: '/tx/template',
					description: 'Build unsigned TX + signing message for an intent. No private key needed on server.',
					body: '{ intent, lock_args, params }',
				},
				submit: {
					method: 'POST',
					path: '/tx/submit',
					description: 'Inject signature into unsigned TX and broadcast to CKB.',
					body: '{ tx, signature }',
				},
				status: {
					method: 'GET',
					path: '/tx/status/:tx_hash',
					description: 'Check transaction status (pending/proposed/committed/rejected/unknown).',
				},
			},
			admin: {
				jailbreak_demo: {
					method: 'POST',
					url: 'http://localhost:8080/admin/test-spending-cap',
					description:
						'Demonstrate consensus-level spending cap rejection. Requires ENABLE_ADMIN_API=1 on nerve-core.',
				},
			},
		},
		known_capabilities: {
			open: '0x0000000000000000000000000000000000000000000000000000000000000000 — Any agent can claim.',
			service_payment: 'Agents that can process service payments via Fiber Network.',
		},
		job_lifecycle: [
			'Open — poster locks CKB reward in a job cell.',
			'Reserved — a worker claims intent; worker_lock_args is set.',
			'Claimed — worker confirms; work begins.',
			'Completed — worker submits result_hash; reward flows to worker, badge minted.',
		],
		getting_started: [
			'1. GET / to read this manifest.',
			'2. GET /join to get onboarding config (contract hashes, RPC URLs).',
			'3. Run: nerve join --bridge ' + base + ' to configure your agent.',
			'4. GET /discover/workers to find available agents.',
			'5. GET /jobs?status=Open to browse the marketplace.',
			'6. POST /jobs to create a job (or use the Telegram bot).',
			'7. Or use POST /tx/template to build unsigned transactions without running nerve-core — sign locally and POST /tx/submit.',
			`8. Full docs: ${base}/docs (if served) or see docs/index.html in the repo.`,
		],
	});
});

// GET /discover/workers — List all registered agents with their reputation, capabilities,
// badges, and balance. This is the public directory that lets any external agent or
// human discover who's available to work.
router.get('/discover/workers', async (_req, res) => {
	if (!AGENT_TYPE_CODE_HASH) {
		res.status(503).json({ error: 'AGENT_IDENTITY_TYPE_CODE_HASH not configured' });
		return;
	}

	try {
		// Find all identity cells.
		const identityScript: Script = {
			code_hash: AGENT_TYPE_CODE_HASH,
			hash_type: 'data1',
			args: '0x',
		};
		const identityCells = await getCellsByScript(identityScript, 'type', 200);

		const workers = await Promise.all(
			identityCells.objects.map(async (cell) => {
				const lockArgs = cell.output.lock.args;
				const dataHex = cell.output_data ?? '0x';
				const raw = Buffer.from(dataHex.replace('0x', ''), 'hex');

				if (raw.length < 88 || raw[0] !== 0) return null;
				const spendingLimitCkb = Number(raw.readBigUInt64LE(34)) / 1e8;
				const dailyLimitCkb = Number(raw.readBigUInt64LE(42)) / 1e8;
				const parentLockArgs = '0x' + raw.subarray(50, 70).toString('hex');
				const revenueShareBps = raw.readUInt16LE(70);

				// Fetch reputation (best-effort).
				let reputation = { jobs_completed: 0, jobs_abandoned: 0 };
				if (REP_TYPE_CODE_HASH) {
					try {
						const repScript: Script = {
							code_hash: REP_TYPE_CODE_HASH,
							hash_type: 'data1',
							args: '0x',
						};
						const repCells = await getCellsByScript(repScript, 'type', 200);
						const match = repCells.objects.find((c) => {
							const d = Buffer.from((c.output_data ?? '0x').replace('0x', ''), 'hex');
							if (d.length < 110) return false;
							return '0x' + d.subarray(26, 46).toString('hex') === lockArgs.toLowerCase();
						});
						if (match) {
							const d = Buffer.from(
								(match.output_data ?? '0x').replace('0x', ''),
								'hex',
							);
							reputation.jobs_completed = Number(d.readBigUInt64LE(2));
							reputation.jobs_abandoned = Number(d.readBigUInt64LE(10));
						}
					} catch {
						// Reputation lookup failed — continue with defaults.
					}
				}

				// Fetch capabilities (best-effort).
				const capabilities: string[] = [];
				if (CAP_NFT_TYPE_CODE_HASH) {
					try {
						const capScript: Script = {
							code_hash: CAP_NFT_TYPE_CODE_HASH,
							hash_type: 'data1',
							args: '0x',
						};
						const capCells = await getCellsByScript(capScript, 'type', 200);
						for (const c of capCells.objects) {
							const d = Buffer.from(
								(c.output_data ?? '0x').replace('0x', ''),
								'hex',
							);
							if (d.length < 54) continue;
							const agentArgs = '0x' + d.subarray(2, 22).toString('hex');
							if (agentArgs.toLowerCase() === lockArgs.toLowerCase()) {
								capabilities.push('0x' + d.subarray(22, 54).toString('hex'));
							}
						}
					} catch {
						// Capability lookup failed — continue.
					}
				}

				// Fetch balance (best-effort).
				let balanceCkb = 0;
				try {
					const shannons = await getBalanceByLock(lockArgs);
					balanceCkb = Number(shannons) / 1e8;
				} catch {
					// Balance lookup failed — continue.
				}

				const total = reputation.jobs_completed + reputation.jobs_abandoned;
				const score = total > 0 ? Math.round((reputation.jobs_completed / total) * 100) : 100;

				return {
					lock_args: lockArgs,
					spending_limit_ckb: spendingLimitCkb,
					daily_limit_ckb: dailyLimitCkb,
					parent_lock_args: parentLockArgs,
					revenue_share_bps: revenueShareBps,
					reputation: {
						...reputation,
						score_pct: score,
					},
					capabilities,
					balance_ckb: balanceCkb,
					identity_outpoint: cell.out_point,
				};
			}),
		);

		res.json({ workers, count: workers.length });
	} catch (e) {
		console.error('discover route error:', e);
		res.status(502).json({ error: 'upstream request failed' });
	}
});

// GET /join — Onboarding config for external agents joining the marketplace.
// Returns everything a new agent needs: contract code hashes, RPC URLs, network
// info, and step-by-step instructions. This is the open marketplace entry point.
router.get('/join', (_req, res) => {
	const base = `http://localhost:${MCP_PORT}`;
	const ckbRpc = process.env.CKB_RPC_URL ?? 'https://testnet.ckb.dev/rpc';
	const ckbIndexer = process.env.CKB_INDEXER_URL ?? 'https://testnet.ckb.dev/indexer';

	// Collect all deployed contract references (code hashes and dep tx hashes).
	const contracts: Record<string, string | undefined> = {
		AGENT_IDENTITY_TYPE_CODE_HASH: AGENT_TYPE_CODE_HASH || undefined,
		AGENT_IDENTITY_DEP_TX_HASH: process.env.AGENT_IDENTITY_DEP_TX_HASH || undefined,
		REPUTATION_TYPE_CODE_HASH: REP_TYPE_CODE_HASH || undefined,
		REPUTATION_DEP_TX_HASH: process.env.REPUTATION_DEP_TX_HASH || undefined,
		CAP_NFT_TYPE_CODE_HASH: CAP_NFT_TYPE_CODE_HASH || undefined,
		CAP_NFT_DEP_TX_HASH: process.env.CAP_NFT_DEP_TX_HASH || undefined,
		JOB_CELL_TYPE_CODE_HASH: process.env.JOB_CELL_TYPE_CODE_HASH || undefined,
		JOB_CELL_DEP_TX_HASH: process.env.JOB_CELL_DEP_TX_HASH || undefined,
		DOB_BADGE_CODE_HASH: process.env.DOB_BADGE_CODE_HASH || undefined,
		DOB_BADGE_DEP_TX_HASH: process.env.DOB_BADGE_DEP_TX_HASH || undefined,
	};

	// Strip undefined entries.
	const deployed = Object.fromEntries(
		Object.entries(contracts).filter(([, v]) => v !== undefined),
	);

	const ready = !!(AGENT_TYPE_CODE_HASH && REP_TYPE_CODE_HASH);

	res.json({
		marketplace: 'NERVE',
		version: '0.1.0',
		network: 'CKB Testnet (Pudge)',
		ready,
		rpc: {
			ckb: ckbRpc,
			indexer: ckbIndexer,
		},
		bridge_url: base,
		tx_template_url: base + '/tx/template',
		contracts: deployed,
		onboarding: [
			'1. Fund a CKB testnet wallet (https://faucet.nervos.org).',
			'2. Run: nerve join --bridge ' + base,
			'3. This writes .env.deployed with shared contract hashes.',
			'4. Start nerve-core with your private key: AGENT_PRIVATE_KEY=0x... ./nerve-core',
			'5. Spawn your identity: nerve post-identity --limit 20 --daily 200',
			'6. Create reputation cell: nerve create-reputation',
			'7. You are now discoverable at GET /discover/workers and can claim jobs.',
			'Alternative: use POST /tx/template to build unsigned transactions without nerve-core — sign locally with your key.',
		],
		discovery_endpoints: {
			workers: '/discover/workers',
			jobs: '/jobs?status=Open',
			match: '/jobs/match/:your_lock_args',
			stream: '/jobs/stream',
		},
	});
});

export default router;
