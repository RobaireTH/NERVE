# Result-Hash Gating for Job Settlement

## Overview

When a job has a non-zero `description_hash`, settlement (destroying a Claimed
cell) requires the worker to provide a result proof in the witness `input_type`
field. The on-chain contract verifies the blake2b binding:

```
blake2b(description_hash || result_data) == result_hash
```

Jobs without descriptions (`description_hash` = zeros) continue to settle
without any proof — fully backward compatible. Description fields become
immutable through all state transitions.

## Result Proof Layout (witness `input_type`)

```
[0..32]    result_hash    [u8;32]   blake2b(description_hash || result_data)
[32..]     result_data    [u8;N]    raw UTF-8 worker result
```

The result_data lives in the witness (no capacity cost). The result_hash is
also stored in a memo cell output (permanent on-chain record).

## Changes

### 1. On-chain contract — `contracts/src/bin/job_cell.rs`

- Update cell data layout comment to include `[90..122] description_hash` and `[122..] description`.
- Change `DATA_MIN` from `90` to `122`.
- Add error codes: `ERR_MISSING_RESULT = 12`, `ERR_INVALID_RESULT_HASH = 13`.
- Import `ckb_hash::blake2b_256` and `high_level::load_witness_args`.
- In `validate_transition`: add immutability check `if old[90..] != new[90..]` (covers both description_hash and variable-length description text).
- In `validate_destruction` for `STATUS_CLAIMED`:
  - Read `description_hash` from `old[90..122]`.
  - If non-zero:
    1. `load_witness_args(0, Source::GroupInput)` → extract `input_type`.
    2. `input_type` must be present and >= 32 bytes.
    3. Parse `result_hash = input_type[0..32]`, `result_data = input_type[32..]`.
    4. Compute `blake2b_256(description_hash || result_data)`.
    5. Verify computed hash == `result_hash`. Reject with `ERR_INVALID_RESULT_HASH` if not.
  - Then call existing `verify_settlement_outputs` (reward check unchanged).

### 2. Rust signing — `packages/core/src/tx_builder/signing.rs`

- Add `placeholder_witness_with_input_type(input_type: &[u8]) -> Vec<u8>`:
  builds a WitnessArgs molecule blob with `lock=Some([0u8;65])` and
  `input_type=Some(input_type)`, `output_type=None`.
- Extract signing logic from `sign_tx` into `sign_with_witness(tx_hash, key, first_witness, additional) -> [u8;65]`. Make `sign_tx` delegate to it.
- Modify `inject_witness` to write the signature into bytes `[20..85]` of the existing `witnesses[0]` instead of rebuilding from scratch (preserves `input_type` data).
- Modify `sign_and_finalize` to read actual witnesses from the TX JSON for signing message computation (instead of hardcoding `placeholder_witness()`).

### 3. Rust complete builder — `packages/core/src/tx_builder/job.rs`

- `build_complete_job`: change parameter from `result_hash: Option<[u8; 32]>` to `result: Option<String>`.
- Read `description_hash` from `job_data[90..122]`.
- If `result` is provided:
  - `result_bytes = result.as_bytes()`
  - `result_hash = blake2b_256(description_hash || result_bytes)`
  - `result_proof = [result_hash(32) || result_bytes]`
  - Use `placeholder_witness_with_input_type(&result_proof)` for `witnesses[0]`.
  - Create memo cell output with `encode_result_memo(&result_hash)` (existing logic).
- If `result` is not provided and `description_hash != [0u8;32]`: return error (result required for described jobs).
- If `result` is not provided and `description_hash == [0u8;32]`: proceed as before (no witness input_type, no memo cell).

### 4. Rust intents — `packages/core/src/tx_builder/intents.rs`

- Change `CompleteJob` variant from `result_hash: Option<String>` to `result: Option<String>`.
- Handler: pass `result` through to `build_complete_job`.

### 5. TypeScript tx-builder — `packages/mcp/src/tx-builder.ts`

- Add `placeholderWitnessWithInputType(inputType: Buffer): Buffer`.
- Fix `buildTemplate`: use actual `tx.witnesses[0]` for signing message (not hardcoded `placeholderWitness()`). Backward compatible — existing witnesses still have lock=zeros.
- Fix `injectSignature`: write signature at bytes `[20..85]` of existing `witnesses[0]` instead of rebuilding.
- `buildCompleteJob`:
  - Accept `result?: string` instead of `result_hash?: string`.
  - Read `description_hash` from `jobData.subarray(90, 122)`.
  - If `result` provided: compute binding hash, build proof, set witness input_type, add memo cell.
  - If `result` not provided and description non-zero: throw error.
  - Add `RESULT_MEMO_CAPACITY` constant and memo cell output (missing from current TS implementation).

## Files modified

| File | Change |
|------|--------|
| `contracts/src/bin/job_cell.rs` | Result-hash verification at settlement, description immutability |
| `packages/core/src/tx_builder/signing.rs` | Witness-with-input-type builder, generic signing |
| `packages/core/src/tx_builder/job.rs` | Complete builder accepts result text, builds proof |
| `packages/core/src/tx_builder/intents.rs` | CompleteJob variant uses `result` field |
| `packages/mcp/src/tx-builder.ts` | Witness-with-input-type, signing fixes, complete builder |

## Not changed

| File | Why |
|------|-----|
| `packages/mcp/src/routes/jobs.ts` | complete_job flows through `/tx/template`, not `/jobs` |
| Reserve/Claim builders | They copy full cell data; description bytes pass through |
| Reputation/Badge systems | They consume `result_hash`; derivation moves to builders |

## Verification

1. `cargo build -p nerve-contracts --target riscv64imac-unknown-none-elf --release` — contract compiles for RISC-V.
2. `cargo test` in `packages/core` — signing roundtrips, encode/decode tests.
3. `npx tsc --noEmit` in `packages/mcp` — clean type-check.
4. Redeploy contract, POST job with description, reserve → claim → complete with result → success.
5. Attempt complete without result on a described job → contract rejects with `ERR_MISSING_RESULT`.
6. Attempt complete on a description-less job → succeeds without result proof (backward compat).

## Commits

1. `feat(contract): enforce result-hash binding and description immutability at settlement.`
2. `feat(core): support result proof in witness for job completion.`
3. `feat(mcp): pass result proof through witness in TypeScript tx-builder.`
