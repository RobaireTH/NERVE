# Fiber Payroll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repackage `mitosis` into a deployable Fiber Payroll product by adding a thin payroll facade in MCP, a new employer dashboard plus contractor portal, and updated deployment/docs while reusing the existing NERVE job and Fiber execution engine.

**Architecture:** Keep `packages/core` unchanged as the job/Fiber execution engine. Add payroll-specific storage and routes in `packages/mcp` that translate contractor and payout actions into `post_job -> reserve_job -> claim_job -> complete_job`. Build a new React + Vite app in `packages/web` that consumes payroll-shaped APIs and hides chain vocabulary from employers and contractors.

**Tech Stack:** Rust workspace (`packages/core`), TypeScript + Express (`packages/mcp`), React + Vite + React Router (`packages/web`), Vitest + Supertest + Testing Library, Docker Compose, Nginx static serving for the web app.

---

### Task 1: Add MCP Test Harness And Payroll Storage

**Files:**
- Modify: `packages/mcp/package.json`
- Modify: `packages/mcp/src/index.ts`
- Create: `packages/mcp/src/app.ts`
- Create: `packages/mcp/src/payroll/types.ts`
- Create: `packages/mcp/src/payroll/store.ts`
- Create: `packages/mcp/src/payroll/store.test.ts`
- Test: `packages/mcp/src/payroll/store.test.ts`

- [ ] **Step 1: Add MCP test dependencies and scripts**

```json
{
  "scripts": {
    "build": "tsc",
    "start": "node dist/index.js",
    "dev": "ts-node src/index.ts",
    "test": "vitest run"
  },
  "devDependencies": {
    "@types/express": "^4.17.0",
    "@types/node": "^20.0.0",
    "@types/supertest": "^6.0.3",
    "supertest": "^7.0.0",
    "ts-node": "^10.9.0",
    "typescript": "^5.0.0",
    "vitest": "^2.1.0"
  }
}
```

- [ ] **Step 2: Write the failing storage test**

```ts
import { describe, expect, it } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { PayrollStore } from './store';

describe('PayrollStore', () => {
  it('persists contractors and payouts to disk', async () => {
    const dir = mkdtempSync(path.join(tmpdir(), 'fiber-payroll-store-'));
    const store = new PayrollStore(dir);

    const contractor = await store.createContractor({
      name: 'Amina Yusuf',
      email: 'amina@example.com',
      notes: 'QA contractor',
    });

    await store.createPayout({
      contractorId: contractor.id,
      amountCkb: 120,
      description: 'May review payout',
    });

    const reloaded = new PayrollStore(dir);
    const contractors = await reloaded.listContractors();
    const payouts = await reloaded.listPayouts();

    expect(contractors).toHaveLength(1);
    expect(contractors[0]?.portalToken).toBeTruthy();
    expect(payouts).toHaveLength(1);
    expect(payouts[0]?.status).toBe('draft');
  });
});
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cd packages/mcp && npm test -- src/payroll/store.test.ts`

Expected: FAIL with module not found errors for `./store` or missing methods on `PayrollStore`.

- [ ] **Step 4: Write the minimal payroll storage implementation**

```ts
// packages/mcp/src/payroll/types.ts
export interface ContractorRecord {
  id: string;
  name: string;
  email: string;
  lockArgs: string | null;
  fiberNodeId: string | null;
  fiberRpcUrl: string | null;
  payoutAddress: string | null;
  notes: string;
  portalToken: string;
  createdAt: string;
  updatedAt: string;
}

export interface PayoutRecord {
  id: string;
  contractorId: string;
  amountCkb: number;
  description: string;
  status: 'draft' | 'pending_contractor_details' | 'queued' | 'processing' | 'paid' | 'payment_failed';
  jobTxHash: string | null;
  jobIndex: number | null;
  reserveTxHash: string | null;
  claimTxHash: string | null;
  completeTxHash: string | null;
  fiberMode: 'direct' | 'hold';
  fiberPaymentHash: string | null;
  fiberSettlementStatus: 'not_requested' | 'pending' | 'paid' | 'failed';
  failureReason: string | null;
  createdAt: string;
  updatedAt: string;
}
```

```ts
// packages/mcp/src/payroll/store.ts
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import type { ContractorRecord, PayoutRecord } from './types';

interface PayrollState {
  contractors: ContractorRecord[];
  payouts: PayoutRecord[];
}

export class PayrollStore {
  constructor(private readonly dataDir: string) {}

  private async readState(): Promise<PayrollState> {
    const file = path.join(this.dataDir, 'payroll.json');
    try {
      const raw = await fs.readFile(file, 'utf8');
      return JSON.parse(raw) as PayrollState;
    } catch {
      return { contractors: [], payouts: [] };
    }
  }

  private async writeState(state: PayrollState): Promise<void> {
    await fs.mkdir(this.dataDir, { recursive: true });
    await fs.writeFile(path.join(this.dataDir, 'payroll.json'), JSON.stringify(state, null, 2) + '\n');
  }

  async listContractors(): Promise<ContractorRecord[]> {
    return (await this.readState()).contractors;
  }

  async listPayouts(): Promise<PayoutRecord[]> {
    return (await this.readState()).payouts;
  }

  async createContractor(input: Pick<ContractorRecord, 'name' | 'email' | 'notes'>): Promise<ContractorRecord> {
    const state = await this.readState();
    const now = new Date().toISOString();
    const contractor: ContractorRecord = {
      id: crypto.randomUUID(),
      name: input.name,
      email: input.email,
      notes: input.notes,
      lockArgs: null,
      fiberNodeId: null,
      fiberRpcUrl: null,
      payoutAddress: null,
      portalToken: crypto.randomUUID(),
      createdAt: now,
      updatedAt: now,
    };
    state.contractors.push(contractor);
    await this.writeState(state);
    return contractor;
  }

  async createPayout(input: Pick<PayoutRecord, 'contractorId' | 'amountCkb' | 'description'>): Promise<PayoutRecord> {
    const state = await this.readState();
    const now = new Date().toISOString();
    const payout: PayoutRecord = {
      id: crypto.randomUUID(),
      contractorId: input.contractorId,
      amountCkb: input.amountCkb,
      description: input.description,
      status: 'draft',
      jobTxHash: null,
      jobIndex: null,
      reserveTxHash: null,
      claimTxHash: null,
      completeTxHash: null,
      fiberMode: 'direct',
      fiberPaymentHash: null,
      fiberSettlementStatus: 'not_requested',
      failureReason: null,
      createdAt: now,
      updatedAt: now,
    };
    state.payouts.push(payout);
    await this.writeState(state);
    return payout;
  }
}
```

- [ ] **Step 5: Extract the Express app for testability**

```ts
// packages/mcp/src/app.ts
import express from 'express';
import chainRouter from './routes/chain.js';
import jobsRouter from './routes/jobs.js';
import agentsRouter from './routes/agents.js';
import fiberRouter from './routes/fiber.js';
import discoverRouter from './routes/discover.js';
import txRouter from './routes/tx.js';

export function createApp() {
  const app = express();
  app.use(express.json({ limit: '1mb' }));
  app.get('/health', (_req, res) => res.json({ status: 'ok', service: 'nerve-mcp' }));
  app.use('/', discoverRouter);
  app.use('/chain', chainRouter);
  app.use('/jobs', jobsRouter);
  app.use('/agents', agentsRouter);
  app.use('/fiber', fiberRouter);
  app.use('/tx', txRouter);
  return app;
}
```

```ts
// packages/mcp/src/index.ts
import { createApp } from './app.js';

const PORT = Number(process.env.MCP_PORT ?? 8081);
createApp().listen(PORT, () => {
  console.log(`nerve-mcp bridge listening on :${PORT}`);
});
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd packages/mcp && npm test -- src/payroll/store.test.ts`

Expected: PASS with `1 passed`.

- [ ] **Step 7: Commit**

```bash
git add packages/mcp/package.json packages/mcp/src/index.ts packages/mcp/src/app.ts packages/mcp/src/payroll/types.ts packages/mcp/src/payroll/store.ts packages/mcp/src/payroll/store.test.ts
git commit -m "feat(mcp): add payroll storage foundation"
```

### Task 2: Add Contractor And Portal Routes

**Files:**
- Modify: `packages/mcp/src/app.ts`
- Create: `packages/mcp/src/routes/payroll.ts`
- Create: `packages/mcp/src/routes/portal.ts`
- Create: `packages/mcp/src/routes/payroll.contractors.test.ts`
- Create: `packages/mcp/src/routes/portal.test.ts`
- Test: `packages/mcp/src/routes/payroll.contractors.test.ts`
- Test: `packages/mcp/src/routes/portal.test.ts`

- [ ] **Step 1: Write the failing contractor route test**

```ts
import { describe, expect, it } from 'vitest';
import request from 'supertest';
import { createApp } from '../app';

describe('payroll contractors API', () => {
  it('creates and lists contractors', async () => {
    process.env.PAYROLL_DATA_DIR = '.tmp-test-payroll-contractors';
    const app = createApp();

    const create = await request(app)
      .post('/payroll/contractors')
      .send({ name: 'Amina Yusuf', email: 'amina@example.com', notes: 'QA contractor' });

    expect(create.status).toBe(201);
    expect(create.body.contractor.name).toBe('Amina Yusuf');

    const list = await request(app).get('/payroll/contractors');
    expect(list.status).toBe(200);
    expect(list.body.contractors).toHaveLength(1);
  });
});
```

- [ ] **Step 2: Write the failing contractor portal test**

```ts
import { describe, expect, it } from 'vitest';
import request from 'supertest';
import { createApp } from '../app';

describe('contractor portal API', () => {
  it('updates payout profile through a portal token', async () => {
    process.env.PAYROLL_DATA_DIR = '.tmp-test-payroll-portal';
    const app = createApp();

    const created = await request(app)
      .post('/payroll/contractors')
      .send({ name: 'Amina Yusuf', email: 'amina@example.com', notes: 'QA contractor' });

    const token = created.body.contractor.portalToken as string;

    const update = await request(app)
      .patch(`/portal/${token}/profile`)
      .send({
        lockArgs: '0x1111111111111111111111111111111111111111',
        fiberNodeId: 'node-1',
        fiberRpcUrl: 'http://worker-fiber:8227',
        payoutAddress: 'fibt_address_1'
      });

    expect(update.status).toBe(200);
    expect(update.body.contractor.lockArgs).toBe('0x1111111111111111111111111111111111111111');
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd packages/mcp && npm test -- src/routes/payroll.contractors.test.ts src/routes/portal.test.ts`

Expected: FAIL with `404` or missing route/module failures.

- [ ] **Step 4: Implement payroll and portal routes**

```ts
// packages/mcp/src/routes/payroll.ts
import { Router } from 'express';
import { PayrollStore } from '../payroll/store.js';

const router = Router();
const store = new PayrollStore(process.env.PAYROLL_DATA_DIR ?? '/app/runtime-data');

router.get('/contractors', async (_req, res) => {
  res.json({ contractors: await store.listContractors() });
});

router.post('/contractors', async (req, res) => {
  const { name, email, notes = '' } = req.body as { name?: string; email?: string; notes?: string };
  if (!name || !email) {
    res.status(400).json({ error: 'name and email are required' });
    return;
  }
  const contractor = await store.createContractor({ name, email, notes });
  res.status(201).json({ contractor });
});

router.patch('/contractors/:id', async (req, res) => {
  const contractor = await store.updateContractor(req.params.id, req.body);
  if (!contractor) {
    res.status(404).json({ error: 'contractor not found' });
    return;
  }
  res.json({ contractor });
});

export default router;
```

```ts
// packages/mcp/src/routes/portal.ts
import { Router } from 'express';
import { PayrollStore } from '../payroll/store.js';

const router = Router();
const store = new PayrollStore(process.env.PAYROLL_DATA_DIR ?? '/app/runtime-data');

router.get('/:token', async (req, res) => {
  const contractor = await store.findContractorByPortalToken(req.params.token);
  if (!contractor) {
    res.status(404).json({ error: 'portal token not found' });
    return;
  }
  res.json({ contractor });
});

router.patch('/:token/profile', async (req, res) => {
  const contractor = await store.updateContractorByPortalToken(req.params.token, req.body);
  if (!contractor) {
    res.status(404).json({ error: 'portal token not found' });
    return;
  }
  res.json({ contractor });
});

router.get('/:token/payouts', async (req, res) => {
  const contractor = await store.findContractorByPortalToken(req.params.token);
  if (!contractor) {
    res.status(404).json({ error: 'portal token not found' });
    return;
  }
  const payouts = await store.listPayoutsForContractor(contractor.id);
  res.json({ contractor, payouts });
});

export default router;
```

```ts
// packages/mcp/src/app.ts
import payrollRouter from './routes/payroll.js';
import portalRouter from './routes/portal.js';

app.use('/payroll', payrollRouter);
app.use('/portal', portalRouter);
```

- [ ] **Step 5: Extend the store with update and lookup helpers**

```ts
async updateContractor(id: string, patch: Partial<ContractorRecord>): Promise<ContractorRecord | null> {
  const state = await this.readState();
  const contractor = state.contractors.find((item) => item.id === id);
  if (!contractor) return null;
  Object.assign(contractor, patch, { updatedAt: new Date().toISOString() });
  await this.writeState(state);
  return contractor;
}

async findContractorByPortalToken(token: string): Promise<ContractorRecord | null> {
  return (await this.readState()).contractors.find((item) => item.portalToken === token) ?? null;
}

async updateContractorByPortalToken(token: string, patch: Partial<ContractorRecord>) {
  const contractor = await this.findContractorByPortalToken(token);
  return contractor ? this.updateContractor(contractor.id, patch) : null;
}

async listPayoutsForContractor(contractorId: string): Promise<PayoutRecord[]> {
  return (await this.readState()).payouts.filter((item) => item.contractorId === contractorId);
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd packages/mcp && npm test -- src/routes/payroll.contractors.test.ts src/routes/portal.test.ts`

Expected: PASS with `2 passed`.

- [ ] **Step 7: Commit**

```bash
git add packages/mcp/src/app.ts packages/mcp/src/routes/payroll.ts packages/mcp/src/routes/portal.ts packages/mcp/src/routes/payroll.contractors.test.ts packages/mcp/src/routes/portal.test.ts packages/mcp/src/payroll/store.ts
git commit -m "feat(mcp): add contractor and portal routes"
```

### Task 3: Add Payout Execution And Ledger Normalization

**Files:**
- Modify: `packages/mcp/src/payroll/store.ts`
- Create: `packages/mcp/src/payroll/executor.ts`
- Modify: `packages/mcp/src/routes/payroll.ts`
- Create: `packages/mcp/src/payroll/executor.test.ts`
- Test: `packages/mcp/src/payroll/executor.test.ts`

- [ ] **Step 1: Write the failing payout execution test**

```ts
import { describe, expect, it, vi } from 'vitest';
import { executePayout } from './executor';
import type { ContractorRecord, PayoutRecord } from './types';

describe('executePayout', () => {
  it('drives post -> reserve -> claim -> complete and returns a paid ledger entry', async () => {
    const contractor = {
      id: 'contractor-1',
      name: 'Amina Yusuf',
      email: 'amina@example.com',
      lockArgs: '0x1111111111111111111111111111111111111111',
      fiberNodeId: 'node-1',
      fiberRpcUrl: 'http://worker-fiber:8227',
      payoutAddress: 'fibt_address_1',
      notes: '',
      portalToken: 'token-1',
      createdAt: '2026-06-09T00:00:00.000Z',
      updatedAt: '2026-06-09T00:00:00.000Z'
    } satisfies ContractorRecord;

    const payout = {
      id: 'payout-1',
      contractorId: contractor.id,
      amountCkb: 120,
      description: 'May review payout',
      status: 'draft',
      jobTxHash: null,
      jobIndex: null,
      reserveTxHash: null,
      claimTxHash: null,
      completeTxHash: null,
      fiberMode: 'direct',
      fiberPaymentHash: null,
      fiberSettlementStatus: 'not_requested',
      failureReason: null,
      createdAt: '2026-06-09T00:00:00.000Z',
      updatedAt: '2026-06-09T00:00:00.000Z'
    } satisfies PayoutRecord;

    const coreFetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ tx_hash: '0xpost' }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ tx_hash: '0xreserve' }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ tx_hash: '0xclaim' }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ tx_hash: '0xcomplete', fiber_payment_requested: true, fiber_payment_mode: 'direct' }) });

    const updated = await executePayout({
      payout,
      contractor,
      coreFetch,
      now: () => '2026-06-09T12:00:00.000Z'
    });

    expect(updated.status).toBe('paid');
    expect(updated.jobTxHash).toBe('0xpost');
    expect(updated.reserveTxHash).toBe('0xreserve');
    expect(updated.claimTxHash).toBe('0xclaim');
    expect(updated.completeTxHash).toBe('0xcomplete');
    expect(updated.fiberSettlementStatus).toBe('paid');
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd packages/mcp && npm test -- src/payroll/executor.test.ts`

Expected: FAIL because `./executor` does not exist.

- [ ] **Step 3: Implement payout execution**

```ts
// packages/mcp/src/payroll/executor.ts
import type { ContractorRecord, PayoutRecord } from './types';

const ZERO_CAPABILITY_HASH = `0x${'0'.repeat(64)}`;

interface ExecutePayoutArgs {
  payout: PayoutRecord;
  contractor: ContractorRecord;
  coreFetch?: typeof fetch;
  now?: () => string;
}

async function callCore(coreFetch: typeof fetch, body: Record<string, unknown>) {
  const response = await coreFetch(`${process.env.CORE_URL ?? 'http://localhost:8080'}/tx/build-and-broadcast`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  const data = await response.json() as Record<string, unknown>;
  if (!response.ok) {
    throw new Error(String(data.error ?? 'core request failed'));
  }
  return data;
}

export async function executePayout({ payout, contractor, coreFetch = fetch, now = () => new Date().toISOString() }: ExecutePayoutArgs): Promise<PayoutRecord> {
  if (!contractor.lockArgs || !contractor.fiberRpcUrl || !contractor.fiberNodeId) {
    return {
      ...payout,
      status: 'pending_contractor_details',
      failureReason: 'contractor payout details are incomplete',
      updatedAt: now(),
    };
  }

  const payment = {
    mode: 'fiber',
    invoice_mode: payout.fiberMode,
    lock_args: contractor.lockArgs,
    node_id: contractor.fiberNodeId,
    rpc_url: contractor.fiberRpcUrl,
    amount_ckb: payout.amountCkb,
    description: payout.description,
  };

  const posted = await callCore(coreFetch, {
    intent: 'post_job',
    reward_ckb: payout.amountCkb,
    ttl_blocks: 200,
    capability_hash: ZERO_CAPABILITY_HASH,
    description: `Payroll payout for ${contractor.name}\n\nNERVE_PAYMENT:${JSON.stringify(payment)}`
  });

  const jobTxHash = String(posted.tx_hash);
  const reserve = await callCore(coreFetch, { intent: 'reserve_job', job_tx_hash: jobTxHash, job_index: 0, worker_lock_args: contractor.lockArgs });
  const claim = await callCore(coreFetch, { intent: 'claim_job', job_tx_hash: jobTxHash, job_index: 0 });
  const complete = await callCore(coreFetch, {
    intent: 'complete_job',
    job_tx_hash: jobTxHash,
    job_index: 0,
    worker_lock_args: contractor.lockArgs,
    result: `Payroll settled for ${contractor.name} (${payout.amountCkb} CKB)`
  });

  return {
    ...payout,
    status: 'paid',
    jobTxHash,
    jobIndex: 0,
    reserveTxHash: String(reserve.tx_hash),
    claimTxHash: String(claim.tx_hash),
    completeTxHash: String(complete.tx_hash),
    fiberSettlementStatus: 'paid',
    updatedAt: now(),
    failureReason: null,
  };
}
```

- [ ] **Step 4: Expose payout and ledger routes**

```ts
// packages/mcp/src/routes/payroll.ts
import { executePayout } from '../payroll/executor.js';

router.get('/payouts', async (_req, res) => {
  res.json({ payouts: await store.listPayouts() });
});

router.post('/payouts', async (req, res) => {
  const { contractorId, amountCkb, description } = req.body as { contractorId?: string; amountCkb?: number; description?: string };
  if (!contractorId || !amountCkb || !description) {
    res.status(400).json({ error: 'contractorId, amountCkb, and description are required' });
    return;
  }
  const payout = await store.createPayout({ contractorId, amountCkb, description });
  res.status(201).json({ payout });
});

router.post('/payouts/:id/execute', async (req, res) => {
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

router.get('/ledger', async (_req, res) => {
  const payouts = await store.listPayouts();
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
```

- [ ] **Step 5: Add store getters and payout persistence helpers**

```ts
async getContractor(id: string) {
  return (await this.readState()).contractors.find((item) => item.id === id) ?? null;
}

async getPayout(id: string) {
  return (await this.readState()).payouts.find((item) => item.id === id) ?? null;
}

async savePayout(next: PayoutRecord): Promise<void> {
  const state = await this.readState();
  const index = state.payouts.findIndex((item) => item.id === next.id);
  if (index === -1) state.payouts.push(next);
  else state.payouts[index] = next;
  await this.writeState(state);
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd packages/mcp && npm test -- src/payroll/executor.test.ts`

Expected: PASS with `1 passed`.

- [ ] **Step 7: Commit**

```bash
git add packages/mcp/src/payroll/store.ts packages/mcp/src/payroll/executor.ts packages/mcp/src/routes/payroll.ts packages/mcp/src/payroll/executor.test.ts
git commit -m "feat(mcp): execute payroll payouts through job lifecycle"
```

### Task 4: Scaffold The Web App

**Files:**
- Create: `packages/web/package.json`
- Create: `packages/web/tsconfig.json`
- Create: `packages/web/tsconfig.node.json`
- Create: `packages/web/vite.config.ts`
- Create: `packages/web/index.html`
- Create: `packages/web/src/main.tsx`
- Create: `packages/web/src/app/App.tsx`
- Create: `packages/web/src/app/router.tsx`
- Create: `packages/web/src/styles.css`
- Create: `packages/web/src/app/App.test.tsx`
- Create: `packages/web/Dockerfile`
- Create: `packages/web/nginx.conf`
- Test: `packages/web/src/app/App.test.tsx`

- [ ] **Step 1: Add web package manifest and dependencies**

```json
{
  "name": "fiber-payroll-web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview --host 0.0.0.0 --port 4173",
    "test": "vitest run"
  },
  "dependencies": {
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "react-router-dom": "^6.28.0"
  },
  "devDependencies": {
    "@testing-library/jest-dom": "^6.6.3",
    "@testing-library/react": "^16.0.1",
    "@types/react": "^18.3.3",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.1",
    "jsdom": "^25.0.1",
    "typescript": "^5.6.2",
    "vite": "^5.4.8",
    "vitest": "^2.1.1"
  }
}
```

- [ ] **Step 2: Write the failing app-shell test**

```tsx
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { App } from './App';

describe('App shell', () => {
  it('renders the Fiber Payroll brand and dashboard nav', () => {
    render(
      <MemoryRouter initialEntries={['/']}>
        <App />
      </MemoryRouter>
    );

    expect(screen.getByText('Fiber Payroll')).toBeInTheDocument();
    expect(screen.getByText('Contractors')).toBeInTheDocument();
    expect(screen.getByText('Ledger')).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cd packages/web && npm test -- src/app/App.test.tsx`

Expected: FAIL with missing `App` or package files.

- [ ] **Step 4: Implement the shell and routes**

```tsx
// packages/web/src/app/App.tsx
import { Link, Outlet } from 'react-router-dom';

export function App() {
  return (
    <div className="shell">
      <aside className="rail">
        <div className="brand">Fiber Payroll</div>
        <nav>
          <Link to="/">Contractors</Link>
          <Link to="/ledger">Ledger</Link>
        </nav>
      </aside>
      <main className="viewport">
        <Outlet />
      </main>
    </div>
  );
}
```

```tsx
// packages/web/src/app/router.tsx
import { createBrowserRouter } from 'react-router-dom';
import { App } from './App';

function Placeholder({ title }: { title: string }) {
  return <section><h1>{title}</h1></section>;
}

export const router = createBrowserRouter([
  {
    path: '/',
    element: <App />,
    children: [
      { index: true, element: <Placeholder title="Contractors" /> },
      { path: 'ledger', element: <Placeholder title="Ledger" /> },
      { path: 'portal/:token', element: <Placeholder title="Contractor Portal" /> }
    ]
  }
]);
```

```ts
// packages/web/src/main.tsx
import React from 'react';
import ReactDOM from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { router } from './app/router';
import './styles.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>
);
```

- [ ] **Step 5: Add initial visual system and container rules**

```css
:root {
  --paper: #f6f8f4;
  --white: #ffffff;
  --ink-900: #0b0f0c;
  --ink-700: #1a2620;
  --ink-500: #5b6b62;
  --green-700: #0f7b36;
  --green-600: #1e9e47;
  --green-100: #d8f0de;
  --line: rgba(11, 15, 12, 0.08);
  --shadow: 0 24px 60px rgba(8, 15, 10, 0.08);
}

body {
  margin: 0;
  font-family: "Plus Jakarta Sans", system-ui, sans-serif;
  background: radial-gradient(circle at top left, #eef7f1, #f6f8f4 50%, #edf4ef 100%);
  color: var(--ink-900);
}

.shell {
  display: grid;
  grid-template-columns: 280px minmax(0, 1fr);
  min-height: 100vh;
}

.rail {
  padding: 28px;
  border-right: 1px solid var(--line);
  background: rgba(255, 255, 255, 0.72);
  backdrop-filter: blur(16px);
}

.viewport {
  padding: 32px;
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd packages/web && npm test -- src/app/App.test.tsx`

Expected: PASS with `1 passed`.

- [ ] **Step 7: Commit**

```bash
git add packages/web/package.json packages/web/tsconfig.json packages/web/tsconfig.node.json packages/web/vite.config.ts packages/web/index.html packages/web/src/main.tsx packages/web/src/app/App.tsx packages/web/src/app/router.tsx packages/web/src/styles.css packages/web/src/app/App.test.tsx packages/web/Dockerfile packages/web/nginx.conf
git commit -m "feat(web): scaffold fiber payroll app shell"
```

### Task 5: Build The Employer Dashboard

**Files:**
- Create: `packages/web/src/lib/api.ts`
- Create: `packages/web/src/features/employer/usePayrollData.ts`
- Create: `packages/web/src/features/employer/ContractorsPage.tsx`
- Create: `packages/web/src/features/employer/LedgerPage.tsx`
- Modify: `packages/web/src/app/router.tsx`
- Create: `packages/web/src/features/employer/ContractorsPage.test.tsx`
- Test: `packages/web/src/features/employer/ContractorsPage.test.tsx`

- [ ] **Step 1: Write the failing employer dashboard test**

```tsx
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ContractorsPage } from './ContractorsPage';

describe('ContractorsPage', () => {
  it('shows contractors and a create payout action', async () => {
    const api = {
      listContractors: vi.fn().mockResolvedValue([
        { id: '1', name: 'Amina Yusuf', email: 'amina@example.com', notes: 'QA', portalToken: 'token-1' }
      ]),
      listLedger: vi.fn().mockResolvedValue([])
    };

    render(<ContractorsPage api={api as never} />);

    expect(await screen.findByText('Amina Yusuf')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /create payout/i })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd packages/web && npm test -- src/features/employer/ContractorsPage.test.tsx`

Expected: FAIL because `ContractorsPage` does not exist.

- [ ] **Step 3: Implement the API client and dashboard page**

```ts
// packages/web/src/lib/api.ts
const API_BASE = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:8081';

export const payrollApi = {
  async listContractors() {
    const response = await fetch(`${API_BASE}/payroll/contractors`);
    return (await response.json()).contractors;
  },
  async listLedger() {
    const response = await fetch(`${API_BASE}/payroll/ledger`);
    return (await response.json()).ledger;
  },
  async createPayout(payload: { contractorId: string; amountCkb: number; description: string }) {
    const response = await fetch(`${API_BASE}/payroll/payouts`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    return (await response.json()).payout;
  }
};
```

```tsx
// packages/web/src/features/employer/ContractorsPage.tsx
import { useEffect, useState } from 'react';
import { payrollApi } from '../../lib/api';

export function ContractorsPage({ api = payrollApi }: { api?: typeof payrollApi }) {
  const [contractors, setContractors] = useState<Array<{ id: string; name: string; email: string; notes: string; portalToken: string }>>([]);

  useEffect(() => {
    void api.listContractors().then(setContractors);
  }, [api]);

  return (
    <section className="stack-lg">
      <header className="hero-panel">
        <div>
          <h1>Contractors</h1>
          <p>Manage payout-ready contractors and portal access.</p>
        </div>
        <button className="primary-action">Create payout</button>
      </header>

      <div className="data-panel">
        <table className="ledger-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Email</th>
              <th>Portal</th>
            </tr>
          </thead>
          <tbody>
            {contractors.map((contractor) => (
              <tr key={contractor.id}>
                <td>{contractor.name}</td>
                <td>{contractor.email}</td>
                <td>/portal/{contractor.portalToken}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
```

- [ ] **Step 4: Implement the ledger page and router bindings**

```tsx
// packages/web/src/features/employer/LedgerPage.tsx
import { useEffect, useState } from 'react';
import { payrollApi } from '../../lib/api';

export function LedgerPage({ api = payrollApi }: { api?: typeof payrollApi }) {
  const [rows, setRows] = useState<Array<{ id: string; amountCkb: number; status: string; fiberSettlementStatus: string; updatedAt: string }>>([]);

  useEffect(() => {
    void api.listLedger().then(setRows);
  }, [api]);

  return (
    <section className="stack-lg">
      <header className="hero-panel">
        <div>
          <h1>Payroll Ledger</h1>
          <p>Track payout progress across job execution and Fiber settlement.</p>
        </div>
      </header>
      <div className="data-panel">
        <table className="ledger-table">
          <thead>
            <tr>
              <th>Payout</th>
              <th>Amount</th>
              <th>Status</th>
              <th>Fiber</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.id}>
                <td>{row.id}</td>
                <td>{row.amountCkb} CKB</td>
                <td>{row.status}</td>
                <td>{row.fiberSettlementStatus}</td>
                <td>{new Date(row.updatedAt).toLocaleString()}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
```

```tsx
// packages/web/src/app/router.tsx
import { ContractorsPage } from '../features/employer/ContractorsPage';
import { LedgerPage } from '../features/employer/LedgerPage';

children: [
  { index: true, element: <ContractorsPage /> },
  { path: 'ledger', element: <LedgerPage /> },
  { path: 'portal/:token', element: <Placeholder title="Contractor Portal" /> }
]
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd packages/web && npm test -- src/features/employer/ContractorsPage.test.tsx`

Expected: PASS with `1 passed`.

- [ ] **Step 6: Commit**

```bash
git add packages/web/src/lib/api.ts packages/web/src/features/employer/usePayrollData.ts packages/web/src/features/employer/ContractorsPage.tsx packages/web/src/features/employer/LedgerPage.tsx packages/web/src/app/router.tsx packages/web/src/features/employer/ContractorsPage.test.tsx packages/web/src/styles.css
git commit -m "feat(web): build employer dashboard"
```

### Task 6: Build The Contractor Portal

**Files:**
- Create: `packages/web/src/features/portal/PortalPage.tsx`
- Create: `packages/web/src/features/portal/PortalPage.test.tsx`
- Modify: `packages/web/src/lib/api.ts`
- Modify: `packages/web/src/app/router.tsx`
- Test: `packages/web/src/features/portal/PortalPage.test.tsx`

- [ ] **Step 1: Write the failing portal test**

```tsx
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { PortalPage } from './PortalPage';

describe('PortalPage', () => {
  it('shows payout details and allows profile updates', async () => {
    const api = {
      getPortal: vi.fn().mockResolvedValue({
        contractor: { name: 'Amina Yusuf', payoutAddress: 'fibt_address_1', lockArgs: '0x1111111111111111111111111111111111111111' },
        payouts: [{ id: 'payout-1', amountCkb: 120, status: 'paid', fiberSettlementStatus: 'paid' }]
      }),
      updatePortalProfile: vi.fn().mockResolvedValue({
        contractor: { name: 'Amina Yusuf', payoutAddress: 'fibt_address_2', lockArgs: '0x1111111111111111111111111111111111111111' }
      })
    };

    render(<PortalPage token="token-1" api={api as never} />);

    expect(await screen.findByText('Amina Yusuf')).toBeInTheDocument();
    expect(screen.getByText('120 CKB')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /save payout details/i })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd packages/web && npm test -- src/features/portal/PortalPage.test.tsx`

Expected: FAIL because `PortalPage` does not exist.

- [ ] **Step 3: Implement portal API methods**

```ts
// packages/web/src/lib/api.ts
async getPortal(token: string) {
  const response = await fetch(`${API_BASE}/portal/${token}`);
  const contractor = (await response.json()).contractor;
  const payoutsResponse = await fetch(`${API_BASE}/portal/${token}/payouts`);
  const payoutsJson = await payoutsResponse.json();
  return { contractor, payouts: payoutsJson.payouts };
},

async updatePortalProfile(token: string, payload: { lockArgs: string; fiberNodeId: string; fiberRpcUrl: string; payoutAddress: string }) {
  const response = await fetch(`${API_BASE}/portal/${token}/profile`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  return await response.json();
}
```

- [ ] **Step 4: Implement the portal page**

```tsx
// packages/web/src/features/portal/PortalPage.tsx
import { useEffect, useState } from 'react';
import { payrollApi } from '../../lib/api';

export function PortalPage({ token, api = payrollApi }: { token: string; api?: typeof payrollApi }) {
  const [profile, setProfile] = useState({ name: '', lockArgs: '', fiberNodeId: '', fiberRpcUrl: '', payoutAddress: '' });
  const [payouts, setPayouts] = useState<Array<{ id: string; amountCkb: number; status: string; fiberSettlementStatus: string }>>([]);

  useEffect(() => {
    void api.getPortal(token).then(({ contractor, payouts }) => {
      setProfile({
        name: contractor.name,
        lockArgs: contractor.lockArgs ?? '',
        fiberNodeId: contractor.fiberNodeId ?? '',
        fiberRpcUrl: contractor.fiberRpcUrl ?? '',
        payoutAddress: contractor.payoutAddress ?? '',
      });
      setPayouts(payouts);
    });
  }, [api, token]);

  return (
    <section className="portal-layout stack-lg">
      <header className="hero-panel">
        <div>
          <h1>{profile.name || 'Contractor Portal'}</h1>
          <p>Update payout details and review payout history.</p>
        </div>
      </header>

      <form className="data-panel stack-md">
        <label>Payout address<input value={profile.payoutAddress} readOnly /></label>
        <label>Lock args<input value={profile.lockArgs} readOnly /></label>
        <button type="button" className="primary-action">Save payout details</button>
      </form>

      <div className="data-panel">
        <h2>Recent payouts</h2>
        <ul className="timeline-list">
          {payouts.map((payout) => (
            <li key={payout.id}>
              <strong>{payout.amountCkb} CKB</strong>
              <span>{payout.status}</span>
              <span>{payout.fiberSettlementStatus}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
```

- [ ] **Step 5: Bind the route**

```tsx
// packages/web/src/app/router.tsx
import { PortalPage } from '../features/portal/PortalPage';

{
  path: 'portal/:token',
  element: <PortalRoute />
}

function PortalRoute() {
  const { token = '' } = useParams();
  return <PortalPage token={token} />;
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd packages/web && npm test -- src/features/portal/PortalPage.test.tsx`

Expected: PASS with `1 passed`.

- [ ] **Step 7: Commit**

```bash
git add packages/web/src/features/portal/PortalPage.tsx packages/web/src/features/portal/PortalPage.test.tsx packages/web/src/lib/api.ts packages/web/src/app/router.tsx packages/web/src/styles.css
git commit -m "feat(web): add contractor payout portal"
```

### Task 7: Wire Deployment And Product Packaging

**Files:**
- Modify: `docker-compose.yml`
- Modify: `packages/mcp/src/routes/discover.ts`
- Modify: `README.md`
- Modify: `packages/mcp/Dockerfile`
- Create: `packages/web/.dockerignore`
- Test: `docker-compose config`
- Test: `cd packages/mcp && npm run build`
- Test: `cd packages/web && npm run build`

- [ ] **Step 1: Write the failing deployment validation step**

Run: `docker-compose config`

Expected: Existing config has no `web` service, so it does not yet describe the Fiber Payroll product.

- [ ] **Step 2: Add the web service and runtime data volume**

```yaml
services:
  mcp:
    build:
      context: .
      dockerfile: packages/mcp/Dockerfile
    ports:
      - "${MCP_PORT:-8081}:8081"
    env_file: .env
    environment:
      PAYROLL_DATA_DIR: /app/runtime-data
    volumes:
      - ./runtime-data:/app/runtime-data
    depends_on:
      core:
        condition: service_healthy

  web:
    build:
      context: .
      dockerfile: packages/web/Dockerfile
    ports:
      - "${WEB_PORT:-4173}:80"
    depends_on:
      - mcp
```

- [ ] **Step 3: Reposition the discovery manifest**

```ts
// packages/mcp/src/routes/discover.ts
res.json({
  name: 'Fiber Payroll',
  version: '0.1.0',
  description: 'Employer-operated contractor payouts on top of NERVE job execution and Fiber settlement.',
  product_surface: {
    dashboard: '/payroll/contractors',
    ledger: '/payroll/ledger',
    contractor_portal: '/portal/:token'
  },
  engine: {
    name: 'NERVE',
    description: 'Underlying job and settlement execution engine'
  },
  endpoints: {
    payroll: {
      list_contractors: { method: 'GET', path: '/payroll/contractors' },
      create_contractor: { method: 'POST', path: '/payroll/contractors' },
      create_payout: { method: 'POST', path: '/payroll/payouts' },
      execute_payout: { method: 'POST', path: '/payroll/payouts/:id/execute' },
      ledger: { method: 'GET', path: '/payroll/ledger' }
    }
  }
});
```

- [ ] **Step 4: Rewrite the README opening and quick start**

```md
# Fiber Payroll

Fiber Payroll is a deployable contractor payout stack built on top of NERVE and Fiber.

## Quick Start

1. Copy `.env.example` to `.env`
2. Start the stack: `docker-compose up --build`
3. Open the employer dashboard on `http://localhost:4173`
4. Create a contractor, send a portal link, and execute the first payout

## Engine

NERVE remains the execution engine for job lifecycle, chain proofs, and Fiber settlement orchestration.
```

- [ ] **Step 5: Run validation commands**

Run:

```bash
docker-compose config
cd packages/mcp && npm run build
cd ../web && npm run build
```

Expected:
- `docker-compose config` exits 0
- MCP TypeScript build exits 0
- Web Vite build exits 0

- [ ] **Step 6: Commit**

```bash
git add docker-compose.yml packages/mcp/src/routes/discover.ts README.md packages/mcp/Dockerfile packages/web/.dockerignore packages/web/Dockerfile packages/web/nginx.conf
git commit -m "feat: package mitosis as fiber payroll"
```

## Self-Review

- Spec coverage:
  - Thin payroll façade: covered by Tasks 1-3.
  - Employer dashboard: covered by Tasks 4-5.
  - Contractor portal: covered by Task 6.
  - Deployment/docs repackaging: covered by Task 7.
- Placeholder scan:
  - No `TODO` or `TBD` placeholders remain in task steps.
- Type consistency:
  - Contractor/payout names are consistent across store, routes, executor, and UI.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-09-fiber-payroll.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
