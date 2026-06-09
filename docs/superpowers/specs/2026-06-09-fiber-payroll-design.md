# Fiber Payroll Design

## Summary

Repackage `mitosis` from a generic NERVE marketplace into `Fiber Payroll`: a deployable payroll stack for small teams paying remote contractors through Fiber-backed payout flows. V1 is employer-operated, payout-on-demand, and infra-led. Employers use a web dashboard to manage contractors and initiate payouts. Contractors get a lightweight portal to update payout details and view payout status.

The implementation should preserve the existing execution engine:

- `nerve-core` remains the transaction builder, signer, and broadcaster.
- `nerve-mcp` remains the integration layer over jobs, agents, chain state, and Fiber RPC.
- Fiber remains the payment rail and settlement visibility layer.

The product shift happens at the API, UI, docs, and packaging layers.

## Product Scope

### Target user

Small teams paying remote contractors in recurring or ad hoc cycles, with an employer-side operator driving payouts manually.

### V1 workflow

1. Employer creates contractor records.
2. Contractor receives a portal link and updates payout details.
3. Employer initiates a payout for a contractor.
4. The system translates that payout into the existing NERVE job lifecycle plus Fiber payment metadata.
5. Employer sees payout progress in a payroll ledger.
6. Contractor sees payout status in the portal.

### Explicitly out of scope for v1

- Full recurring payroll scheduling
- Tax calculations or compliance workflows
- Multi-employer tenancy hardening
- Full contractor account system
- Replacing the underlying NERVE job lifecycle with a separate payroll protocol

## Architecture

### Product positioning

`Fiber Payroll` is the product surface. `NERVE` is the engine beneath it.

The repo should be packaged as:

- `packages/core`: unchanged execution engine
- `packages/mcp`: existing bridge plus a new payroll façade
- `packages/web`: new dashboard and contractor portal
- top-level docs and docker config updated to describe payroll deployment first

### Thin payroll façade

V1 should use a thin façade over existing primitives instead of introducing a new standalone payroll backend abstraction.

The façade translates:

- contractor record -> payout destination + lock args metadata
- payout -> job cell + Fiber payment metadata
- payroll ledger entry -> normalized view of job lifecycle + Fiber payment state
- contractor portal session -> tokenized access to contractor profile and payout history

This keeps the system infra-led while removing marketplace vocabulary from the user-facing product.

### Underlying payout state model

Each payout uses the existing job lifecycle:

1. `post_job` creates the payout job with Fiber payment metadata embedded in description metadata.
2. `reserve_job` assigns the contractor worker lock args.
3. `claim_job` marks the payout as in progress at the job layer.
4. `complete_job` finalizes the job and triggers Fiber settlement logic already present in MCP.

The payroll façade is responsible for presenting these states as:

- `draft`
- `pending_contractor_details`
- `queued`
- `processing`
- `paid`
- `payment_failed`

The façade should not expose `Open`, `Reserved`, `Claimed`, or `Completed` directly in the product UI.

## Components

### 1. Payroll API in `packages/mcp`

Add payroll-shaped endpoints inside MCP instead of building a new backend service.

Core responsibilities:

- create and list contractors
- update contractor payout details
- create payouts
- execute payout lifecycle using existing job intents
- aggregate ledger data from jobs plus Fiber state
- issue lightweight contractor portal tokens
- expose portal-specific read/update endpoints

The API may persist lightweight application state in local JSON storage for v1 rather than adding a database. This is acceptable because the repo currently has no application database layer and the goal is packaging speed with deployability.

### 2. Employer dashboard in `packages/web`

The employer dashboard should provide:

- contractors table
- contractor detail panel
- create payout flow
- payout ledger
- payout activity and failure states

The dashboard should emphasize operational visibility:

- current payout status
- Fiber payment state
- job proof / chain trace where useful
- failed or blocked payout reasons

It should look like payroll software, not a hackathon chain explorer.

### 3. Contractor portal in `packages/web`

The contractor portal should provide:

- invite/token entry
- contractor profile summary
- payout destination form
- payout history
- payout status timeline

No full auth system is required in v1. Tokenized access links are sufficient.

## Data Model

### Contractor

V1 contractor records should include:

- `id`
- `name`
- `email`
- `lockArgs` optional
- `fiberNodeId` optional
- `fiberRpcUrl` optional
- `payoutAddress` optional
- `notes`
- `portalToken`
- timestamps

### Payout

V1 payouts should include:

- `id`
- `contractorId`
- `amountCkb`
- `description`
- `status`
- `jobTxHash` optional
- `jobIndex` optional
- `reserveTxHash` optional
- `claimTxHash` optional
- `completeTxHash` optional
- `fiberMode`
- `fiberPaymentHash` optional
- `fiberSettlementStatus`
- `failureReason` optional
- timestamps

### Storage

Use file-backed JSON storage in `packages/mcp` for v1:

- contractor store
- payout store
- portal token lookup

The storage boundary should be encapsulated so later migration to SQLite or Postgres is straightforward.

## API Design

### Employer-facing routes

Add routes such as:

- `GET /payroll/contractors`
- `POST /payroll/contractors`
- `PATCH /payroll/contractors/:id`
- `GET /payroll/payouts`
- `POST /payroll/payouts`
- `POST /payroll/payouts/:id/execute`
- `GET /payroll/ledger`

### Contractor portal routes

- `GET /portal/:token`
- `PATCH /portal/:token/profile`
- `GET /portal/:token/payouts`

### Execution semantics

`POST /payroll/payouts/:id/execute` should:

1. validate contractor payout readiness
2. create the underlying job
3. reserve the job with the contractor lock args
4. claim the job
5. complete the job with a payroll settlement result string
6. collect Fiber outcome and persist the ledger state

If a step fails, the payout record should persist the failure state and returned details.

## Deployment

### Services

V1 deployment should extend current compose-based packaging with:

- existing `core`
- existing `mcp`
- existing `fiber`
- new `web`

The `agent` service is not central to the payroll product and should be optional or de-emphasized in deployment docs.

### Packaging story

The README and compose experience should present:

1. start Fiber Payroll locally
2. configure employer environment
3. invite contractor
4. send first payout

The older agent-marketplace framing should move to secondary or engine-level documentation.

## Error Handling

### Employer-facing errors

Convert low-level failures into operational messages:

- missing contractor payout details
- missing Fiber route configuration
- job lifecycle transition failure
- Fiber settlement failed
- payout recorded but not settled

### Contractor-facing errors

Keep contractor portal messaging minimal and actionable:

- invalid invite link
- payout details incomplete
- payout pending employer action

## Testing

### Backend

Test:

- payroll record storage
- contractor CRUD validation
- payout execution lifecycle
- status normalization
- portal token lookup
- failure propagation

### Frontend

Test:

- dashboard renders contractor and payout data
- create payout flow
- contractor portal profile update flow
- responsive layout for dashboard and portal

### Integration

Verify:

- web -> payroll façade -> existing MCP/core routes
- payout execution updates ledger state
- Fiber success and failure states are visible in UI

## Trade-offs

### Why thin façade

Chosen because:

- fastest path to a usable payroll product
- preserves current engine and contract assumptions
- avoids inventing a second orchestration layer before product validation

### Known limitations

- file storage is not ideal for multi-user production
- existing job semantics remain visible internally
- Fiber routing failures may still surface due to current network constraints

These are acceptable for v1 packaging if the UI and docs make the product coherent.
