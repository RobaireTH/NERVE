# Commit Organization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Organize and commit all changes in the `mitosis/` directory from June 2 to June 9 into logical, balanced groups.

**Architecture:** We will group changes by subsystem: Infrastructure/Rebranding, Core Signing, MCP Infrastructure, Payroll Features, Portal Features, and Web Dashboard/Documentation.

**Tech Stack:** Git, Rust, Node.js (MCP), React (Web).

---

### Task 1: Infrastructure & Rebranding

**Files:**
- Modify: `mitosis/.gitignore`
- Modify: `mitosis/README.md`
- Modify: `mitosis/docker-compose.yml`
- Modify: `mitosis/scripts/start_nerve.sh`
- Modify: `mitosis/scripts/test_integration.sh`
- Modify: `mitosis/packages/agent/openclaw.json`

- [ ] **Step 1: Stage infrastructure files**

Run: `git add .gitignore README.md docker-compose.yml scripts/start_nerve.sh scripts/test_integration.sh packages/agent/openclaw.json`

- [ ] **Step 2: Commit infrastructure changes**

```bash
git commit -m "chore: rebrand to Fiber Payroll and update infrastructure

- Update README.md with Fiber Payroll focus
- Add web service to docker-compose.yml
- Update scripts for integration testing and startup
- Adjust OpenClaw agent configuration"
```

---

### Task 2: Core Signing & Identity Improvements

**Files:**
- Modify: `mitosis/packages/core/src/signer.rs`
- Modify: `mitosis/packages/core/src/tx_builder/identity.rs`

- [ ] **Step 1: Stage core files**

Run: `git add packages/core/src/signer.rs packages/core/src/tx_builder/identity.rs`

- [ ] **Step 2: Commit core changes**

```bash
git commit -m "feat(core): improve signer and identity management

- Add support for MCP tool calls in SuperiseSigner
- Refactor identity derivation logic
- Add validation for CKB signing compatibility"
```

---

### Task 3: MCP Infrastructure & Discovery

**Files:**
- Modify: `mitosis/packages/mcp/package.json`
- Modify: `mitosis/packages/mcp/package-lock.json`
- Modify: `mitosis/packages/mcp/tsconfig.json`
- Modify: `mitosis/packages/mcp/src/index.ts`
- Modify: `mitosis/packages/mcp/src/routes/discover.ts`
- Create: `mitosis/packages/mcp/src/app.ts`

- [ ] **Step 1: Stage MCP infra files**

Run: `git add packages/mcp/package.json packages/mcp/package-lock.json packages/mcp/tsconfig.json packages/mcp/src/index.ts packages/mcp/src/routes/discover.ts packages/mcp/src/app.ts`

- [ ] **Step 2: Commit MCP infra changes**

```bash
git commit -m "feat(mcp): update infrastructure and discovery routes

- Add new dependencies to package.json
- Refactor index.ts to use separate app.ts
- Enhance discovery route for tool listing
- Update tsconfig for better type checking"
```

---

### Task 4: Payroll Feature Implementation

**Files:**
- Create: `mitosis/packages/mcp/src/payroll/`
- Create: `mitosis/packages/mcp/src/routes/payroll.ts`
- Create: `mitosis/packages/mcp/src/routes/payroll.contractors.test.ts`

- [ ] **Step 1: Stage payroll files**

Run: `git add packages/mcp/src/payroll/ packages/mcp/src/routes/payroll.ts packages/mcp/src/routes/payroll.contractors.test.ts`

- [ ] **Step 2: Commit payroll changes**

```bash
git commit -m "feat(mcp): implement payroll management features

- Add payroll store and executor logic
- Add routes for contractor and payroll management
- Add unit tests for contractor payroll flows"
```

---

### Task 5: Portal Feature & CLI Scripts

**Files:**
- Create: `mitosis/packages/mcp/src/routes/portal.ts`
- Create: `mitosis/packages/mcp/src/routes/portal.test.ts`
- Create: `mitosis/packages/mcp/src/test/`
- Modify: `mitosis/scripts/nerve`

- [ ] **Step 1: Stage portal and script files**

Run: `git add packages/mcp/src/routes/portal.ts packages/mcp/src/routes/portal.test.ts packages/mcp/src/test/ scripts/nerve`

- [ ] **Step 2: Commit portal changes**

```bash
git commit -m "feat(mcp): add contractor portal and update CLI scripts

- Implement portal routes for contractor access
- Add integration tests for portal functionality
- Update nerve CLI script with new commands"
```

---

### Task 6: Web Dashboard & Documentation

**Files:**
- Create: `mitosis/packages/web/`
- Create: `mitosis/INTEGRATION_SUMMARY.md`
- Create: `mitosis/LIMITATIONS.md`
- Create: `mitosis/TEST.md`
- Create: `mitosis/hackathon-ckb.md`
- Create: `mitosis/implementation-plan.md`
- Create: `mitosis/submission-draft.md`
- Create: `mitosis/submission-draft.zh.md`
- Create: `mitosis/docs/superpowers/plans/`
- Create: `mitosis/pages/contents.md`
- Create: `mitosis/keepthis.md`

- [ ] **Step 1: Stage web and doc files**

Run: `git add packages/web/ INTEGRATION_SUMMARY.md LIMITATIONS.md TEST.md hackathon-ckb.md implementation-plan.md submission-draft.md submission-draft.zh.md docs/superpowers/plans/ pages/contents.md keepthis.md`

- [ ] **Step 2: Commit web and doc changes**

```bash
git commit -m "feat: add web dashboard and comprehensive documentation

- Implement React-based employer and portal dashboard
- Add project integration summary and limitations
- Add hackathon submission drafts and implementation plans
- Organize project documentation"
```
