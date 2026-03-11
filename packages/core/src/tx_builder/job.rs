use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{
		parse_capacity_hex, AppState, SECP256K1_CODE_HASH, SECP256K1_DEP_TX_HASH,
		SECP256K1_HASH_TYPE,
	},
};

use super::signing::{inject_witness, placeholder_witness, sign_tx};

// Job status bytes.
pub const STATUS_OPEN: u8 = 0;
pub const STATUS_RESERVED: u8 = 1;
pub const STATUS_CLAIMED: u8 = 2;

// Minimum capacity for a job cell:
//   cap(8) + lock(53) + type(33) + data(90) = 184 bytes → 184 CKB.
pub const JOB_CELL_OVERHEAD: u64 = 184 * 100_000_000;

// Minimum capacity for a plain secp256k1 payment cell (no type, no data).
pub const MIN_PAYMENT_CELL: u64 = 61 * 100_000_000;

const ESTIMATED_FEE: u64 = 2_000_000;

fn job_type_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("JOB_CELL_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"JOB_CELL_TYPE_CODE_HASH not set — run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	let dep_tx_hash = std::env::var("JOB_CELL_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"JOB_CELL_DEP_TX_HASH not set — run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	Ok((code_hash, dep_tx_hash))
}

/// Encodes the 90-byte job cell data field.
pub fn encode_job_data(
	poster_lock_args: &[u8; 20],
	worker_lock_args: &[u8; 20],
	reward_shannons: u64,
	ttl_block_height: u64,
	capability_hash: &[u8; 32],
) -> Vec<u8> {
	let mut data = Vec::with_capacity(90);
	data.push(0u8); // version
	data.push(STATUS_OPEN);
	data.extend_from_slice(poster_lock_args);
	data.extend_from_slice(worker_lock_args);
	data.extend_from_slice(&reward_shannons.to_le_bytes());
	data.extend_from_slice(&ttl_block_height.to_le_bytes());
	data.extend_from_slice(capability_hash);
	data
}

/// Parses lock_args hex (0x-prefixed) to [u8; 20].
pub fn parse_lock_args_20(hex: &str) -> Result<[u8; 20], TxBuildError> {
	let bytes = hex::decode(hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::InvalidLockArgs(e.to_string()))?;
	if bytes.len() != 20 {
		return Err(TxBuildError::InvalidLockArgs(format!(
			"expected 20 bytes, got {}",
			bytes.len()
		)));
	}
	let mut arr = [0u8; 20];
	arr.copy_from_slice(&bytes);
	Ok(arr)
}

/// Parses a 32-byte hash hex (0x-prefixed) to [u8; 32].
pub fn parse_hash_32(hex: &str) -> Result<[u8; 32], TxBuildError> {
	let bytes = hex::decode(hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::InvalidTypeArgs(e.to_string()))?;
	if bytes.len() != 32 {
		return Err(TxBuildError::InvalidTypeArgs(format!(
			"expected 32 bytes, got {}",
			bytes.len()
		)));
	}
	let mut arr = [0u8; 32];
	arr.copy_from_slice(&bytes);
	Ok(arr)
}

/// Fetches a live job cell and parses its data. Returns (cell_capacity, job_data_bytes).
async fn fetch_job_cell(
	state: &AppState,
	tx_hash: &str,
	index: u32,
) -> Result<(u64, Vec<u8>), TxBuildError> {
	let result = state.ckb.get_live_cell(tx_hash, index).await?;

	if result.status != "live" {
		return Err(TxBuildError::CellNotFound(format!(
			"{}:{} status={}",
			tx_hash, index, result.status
		)));
	}

	let cell = result
		.cell
		.ok_or_else(|| TxBuildError::CellNotFound(format!("{tx_hash}:{index}")))?;

	let capacity = parse_capacity_hex(&cell.output.capacity)?;

	let data_hex = cell
		.data
		.map(|d| d.content)
		.unwrap_or_else(|| "0x".into());
	let data = hex::decode(data_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad cell data: {e}")))?;

	if data.len() < 90 {
		return Err(TxBuildError::Rpc("job cell data too short".into()));
	}

	Ok((capacity, data))
}

/// Gathers enough of the agent's own secp256k1 cells to cover `needed` shannons (fee source).
async fn gather_fee_inputs(
	state: &AppState,
	needed: u64,
) -> Result<(Vec<Value>, u64), TxBuildError> {
	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};
	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut capacity: u64 = 0;
	for cell in &cells.objects {
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		inputs.push(json!({ "previous_output": cell.out_point, "since": "0x0" }));
		capacity += cap;
		if capacity >= needed + MIN_PAYMENT_CELL {
			break;
		}
	}
	if capacity < needed + MIN_PAYMENT_CELL {
		return Err(TxBuildError::InsufficientFunds {
			need: (needed + MIN_PAYMENT_CELL) as f64 / 1e8,
			have: capacity as f64 / 1e8,
		});
	}
	Ok((inputs, capacity))
}

fn our_lock(state: &AppState) -> Script {
	Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	}
}

async fn sign_and_finalize(
	state: &AppState,
	mut tx: Value,
) -> Result<(Value, String), TxBuildError> {
	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();
	let signature = sign_tx(&tx_hash, &state.private_key)?;
	inject_witness(&mut tx, &signature);
	Ok((tx, tx_hash))
}

fn placeholder_witnesses(count: usize) -> Vec<Value> {
	let ph = format!("0x{}", hex::encode(placeholder_witness()));
	(0..count)
		.map(|i| {
			if i == 0 {
				serde_json::Value::String(ph.clone())
			} else {
				serde_json::Value::String("0x".into())
			}
		})
		.collect()
}

// ──────────────────────────────────────────────────────────────────────────────
// Public intent builders
// ──────────────────────────────────────────────────────────────────────────────

/// POST /tx/build {intent: "post_job"} — creates an Open job cell.
pub async fn build_post_job(
	state: &AppState,
	reward_shannons: u64,
	ttl_blocks: u64,
	capability_hash: [u8; 32],
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = job_type_env()?;

	let tip = state.ckb.get_tip_block_number().await?;
	let ttl_block_height = tip + ttl_blocks;

	let poster_lock_args = parse_lock_args_20(&state.lock_args)?;
	let job_data = encode_job_data(
		&poster_lock_args,
		&[0u8; 20],
		reward_shannons,
		ttl_block_height,
		&capability_hash,
	);

	let job_cell_capacity = JOB_CELL_OVERHEAD + reward_shannons;
	let (fee_inputs, fee_capacity) =
		gather_fee_inputs(state, job_cell_capacity + ESTIMATED_FEE).await?;

	let change_capacity = fee_capacity - job_cell_capacity - ESTIMATED_FEE;
	let witnesses = placeholder_witnesses(fee_inputs.len());

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": fee_inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", job_cell_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{ "capacity": format!("{:#x}", change_capacity), "lock": our_lock(state), "type": null },
		],
		"outputs_data": [format!("0x{}", hex::encode(&job_data)), "0x"],
		"witnesses": witnesses,
	});

	sign_and_finalize(state, tx).await
}

/// reserve_job — transitions Open → Reserved, sets worker_lock_args.
pub async fn build_reserve_job(
	state: &AppState,
	job_tx_hash: &str,
	job_index: u32,
	worker_lock_args: &str,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = job_type_env()?;
	let (job_capacity, mut job_data) = fetch_job_cell(state, job_tx_hash, job_index).await?;

	if job_data[1] != STATUS_OPEN {
		return Err(TxBuildError::Rpc(format!("job status is {}, expected Open(0)", job_data[1])));
	}

	let worker_bytes = parse_lock_args_20(worker_lock_args)?;
	job_data[1] = STATUS_RESERVED;
	job_data[22..42].copy_from_slice(&worker_bytes);

	let (fee_inputs, fee_capacity) = gather_fee_inputs(state, ESTIMATED_FEE).await?;
	let change_capacity = fee_capacity - ESTIMATED_FEE;

	let mut all_inputs = vec![json!({ "previous_output": { "tx_hash": job_tx_hash, "index": format!("{:#x}", job_index) }, "since": "0x0" })];
	all_inputs.extend(fee_inputs);
	let witnesses = placeholder_witnesses(all_inputs.len());

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": all_inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", job_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{ "capacity": format!("{:#x}", change_capacity), "lock": our_lock(state), "type": null },
		],
		"outputs_data": [format!("0x{}", hex::encode(&job_data)), "0x"],
		"witnesses": witnesses,
	});

	sign_and_finalize(state, tx).await
}

/// claim_job — transitions Reserved → Claimed.
pub async fn build_claim_job(
	state: &AppState,
	job_tx_hash: &str,
	job_index: u32,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = job_type_env()?;
	let (job_capacity, mut job_data) = fetch_job_cell(state, job_tx_hash, job_index).await?;

	if job_data[1] != STATUS_RESERVED {
		return Err(TxBuildError::Rpc(format!(
			"job status is {}, expected Reserved(1)",
			job_data[1]
		)));
	}
	job_data[1] = STATUS_CLAIMED;

	let (fee_inputs, fee_capacity) = gather_fee_inputs(state, ESTIMATED_FEE).await?;
	let change_capacity = fee_capacity - ESTIMATED_FEE;

	let mut all_inputs = vec![json!({ "previous_output": { "tx_hash": job_tx_hash, "index": format!("{:#x}", job_index) }, "since": "0x0" })];
	all_inputs.extend(fee_inputs);
	let witnesses = placeholder_witnesses(all_inputs.len());

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": all_inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", job_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{ "capacity": format!("{:#x}", change_capacity), "lock": our_lock(state), "type": null },
		],
		"outputs_data": [format!("0x{}", hex::encode(&job_data)), "0x"],
		"witnesses": witnesses,
	});

	sign_and_finalize(state, tx).await
}

/// complete_job — destroys the job cell, routes reward to worker and overhead back to poster.
pub async fn build_complete_job(
	state: &AppState,
	job_tx_hash: &str,
	job_index: u32,
	worker_lock_args: &str,
) -> Result<(Value, String), TxBuildError> {
	let (_, dep_tx_hash) = job_type_env()?;
	let (job_capacity, job_data) = fetch_job_cell(state, job_tx_hash, job_index).await?;

	if job_data[1] != STATUS_CLAIMED {
		return Err(TxBuildError::Rpc(format!(
			"job status is {}, expected Claimed(2)",
			job_data[1]
		)));
	}

	let reward_shannons = u64::from_le_bytes(job_data[42..50].try_into().unwrap());
	let poster_refund = job_capacity - reward_shannons;

	// Verify reward is large enough for a standalone cell.
	if reward_shannons < MIN_PAYMENT_CELL {
		return Err(TxBuildError::InsufficientCapacity {
			need: MIN_PAYMENT_CELL,
			have: reward_shannons,
		});
	}

	// Build worker payment cell lock.
	let worker_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: worker_lock_args.into(),
	};

	// Fee comes from the overhead reduction (poster_refund > 61 CKB; fee is ~0.01 CKB).
	let poster_refund_after_fee = poster_refund - ESTIMATED_FEE;

	let inputs = vec![json!({ "previous_output": { "tx_hash": job_tx_hash, "index": format!("{:#x}", job_index) }, "since": "0x0" })];
	let witnesses = placeholder_witnesses(inputs.len());

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			// Reward to worker.
			{ "capacity": format!("{:#x}", reward_shannons), "lock": worker_lock, "type": null },
			// Overhead refund to poster.
			{ "capacity": format!("{:#x}", poster_refund_after_fee), "lock": our_lock(state), "type": null },
		],
		"outputs_data": ["0x", "0x"],
		"witnesses": witnesses,
	});

	sign_and_finalize(state, tx).await
}

/// cancel_job — destroys the job cell (Expired), returns capacity to poster.
pub async fn build_cancel_job(
	state: &AppState,
	job_tx_hash: &str,
	job_index: u32,
) -> Result<(Value, String), TxBuildError> {
	let (_, dep_tx_hash) = job_type_env()?;
	let (job_capacity, job_data) = fetch_job_cell(state, job_tx_hash, job_index).await?;

	let status = job_data[1];
	if status != STATUS_OPEN && status != STATUS_RESERVED {
		return Err(TxBuildError::Rpc(format!(
			"job status is {}, expected Open(0) or Reserved(1) to cancel",
			status
		)));
	}

	let recovery = job_capacity - ESTIMATED_FEE;

	let inputs = vec![json!({ "previous_output": { "tx_hash": job_tx_hash, "index": format!("{:#x}", job_index) }, "since": "0x0" })];
	let witnesses = placeholder_witnesses(inputs.len());

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			{ "capacity": format!("{:#x}", recovery), "lock": our_lock(state), "type": null },
		],
		"outputs_data": ["0x"],
		"witnesses": witnesses,
	});

	sign_and_finalize(state, tx).await
}
