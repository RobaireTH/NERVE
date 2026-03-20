use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{
		parse_capacity_hex, AppState, SECP256K1_CODE_HASH, SECP256K1_DEP_TX_HASH,
		SECP256K1_HASH_TYPE,
	},
	tx_builder::identity::{blake2b_256, decode_identity_data, IdentityData},
};

use super::molecule::compute_raw_tx_hash;
use super::signing::{
	inject_witness, placeholder_witness, placeholder_witness_with_input_type,
};
use super::{gather_fee_inputs, our_lock, placeholder_witnesses};

pub const STATUS_OPEN: u8 = 0;
pub const STATUS_RESERVED: u8 = 1;
pub const STATUS_CLAIMED: u8 = 2;

// Minimum capacity for a job cell:
//   cap(8) + lock(53) + type(33) + data(122) = 216 bytes → 216 CKB.
//   Data grew from 90 to 122 bytes with the addition of description_hash[32].
pub const JOB_CELL_OVERHEAD: u64 = 216 * 100_000_000;

// Minimum capacity for a plain secp256k1 payment cell (no type, no data).
pub const MIN_PAYMENT_CELL: u64 = 61 * 100_000_000;

// Minimum capacity for a result memo cell: cap(8) + lock(53) + data(33) = 94 → round to 97 CKB.
pub const RESULT_MEMO_CAPACITY: u64 = 97 * 100_000_000;

const ESTIMATED_FEE: u64 = 2_000_000;

fn job_type_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("JOB_CELL_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"JOB_CELL_TYPE_CODE_HASH not set; run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	let dep_tx_hash = std::env::var("JOB_CELL_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"JOB_CELL_DEP_TX_HASH not set; run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	Ok((code_hash, dep_tx_hash))
}

pub fn encode_job_data(
	poster_lock_args: &[u8; 20],
	worker_lock_args: &[u8; 20],
	reward_shannons: u64,
	ttl_block_height: u64,
	capability_hash: &[u8; 32],
	description_hash: &[u8; 32],
	description: &[u8],
) -> Vec<u8> {
	let mut data = Vec::with_capacity(122 + description.len());
	data.push(0u8);
	data.push(STATUS_OPEN);
	data.extend_from_slice(poster_lock_args);
	data.extend_from_slice(worker_lock_args);
	data.extend_from_slice(&reward_shannons.to_le_bytes());
	data.extend_from_slice(&ttl_block_height.to_le_bytes());
	data.extend_from_slice(capability_hash);
	data.extend_from_slice(description_hash);
	data.extend_from_slice(description);
	data
}

pub fn encode_result_memo(result_hash: &[u8; 32]) -> Vec<u8> {
	let mut data = Vec::with_capacity(33);
	data.push(0u8);
	data.extend_from_slice(result_hash);
	data
}

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

async fn sign_and_finalize(
	state: &AppState,
	mut tx: Value,
) -> Result<(Value, String), TxBuildError> {
	let witnesses = tx["witnesses"]
		.as_array()
		.ok_or_else(|| TxBuildError::Signing("missing witnesses array".into()))?;
	let witness_count = witnesses.len().max(1);

	let first_hex = witnesses
		.first()
		.and_then(|v| v.as_str())
		.unwrap_or("0x");
	let first_witness = hex::decode(first_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Signing(format!("bad witness hex: {e}")))?;

	let tx_hash = compute_raw_tx_hash(&tx)?;
	let signature = state.signer.sign_with_witness(&tx_hash, &first_witness, witness_count).await?;
	inject_witness(&mut tx, &signature);
	Ok((tx, tx_hash))
}

pub async fn build_post_job(
	state: &AppState,
	reward_shannons: u64,
	ttl_blocks: u64,
	capability_hash: [u8; 32],
	description: Option<String>,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = job_type_env()?;

	let tip = state.ckb.get_tip_block_number().await?;
	let ttl_block_height = tip + ttl_blocks;

	let (desc_hash, desc_bytes) = match &description {
		Some(text) => (blake2b_256(text.as_bytes()), text.as_bytes().to_vec()),
		None => ([0u8; 32], Vec::new()),
	};

	let poster_lock_args = parse_lock_args_20(&state.lock_args)?;
	let job_data = encode_job_data(
		&poster_lock_args,
		&[0u8; 20],
		reward_shannons,
		ttl_block_height,
		&capability_hash,
		&desc_hash,
		&desc_bytes,
	);

	let description_capacity = desc_bytes.len() as u64 * 100_000_000;
	let job_cell_capacity = JOB_CELL_OVERHEAD + reward_shannons + description_capacity;
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

/// Includes header_dep for on-chain TTL validation and, when the job requires a capability,
/// the worker's matching capability NFT as a cell_dep for on-chain verification.
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

	// Header dep for TTL validation in the type script.
	let tip_header_hash = state.ckb.get_tip_header_hash().await?;

	let mut cell_deps = vec![
		json!({ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" }),
		json!({ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" }),
	];

	// If the job requires a capability, find the worker's matching NFT and add as cell_dep.
	let cap_hash = &job_data[58..90];
	if !cap_hash.iter().all(|&b| b == 0) {
		let nft_outpoint = find_worker_capability_nft(state, worker_lock_args, cap_hash).await?;
		cell_deps.push(json!({ "out_point": nft_outpoint, "dep_type": "code" }));
	}

	let tx = json!({
		"version": "0x0",
		"cell_deps": cell_deps,
		"header_deps": [tip_header_hash],
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

/// Returns None if no identity cell is found or the type script env vars are not set.
async fn fetch_worker_identity(
	state: &AppState,
	worker_lock_args: &str,
) -> Result<Option<IdentityData>, TxBuildError> {
	let type_code_hash = match std::env::var("AGENT_IDENTITY_TYPE_CODE_HASH") {
		Ok(v) => v,
		Err(_) => return Ok(None),
	};

	let type_script = Script {
		code_hash: type_code_hash,
		hash_type: "data1".into(),
		args: "0x".into(),
	};

	let cells = state.ckb.get_cells_by_type_script(&type_script, 200).await?;

	let identity_cell = cells
		.objects
		.iter()
		.find(|c| c.output.lock.args.to_lowercase() == worker_lock_args.to_lowercase());

	let Some(cell) = identity_cell else {
		return Ok(None);
	};

	// Fetch full cell data via get_live_cell.
	let live = state
		.ckb
		.get_live_cell(&cell.out_point.tx_hash, {
			u32::from_str_radix(cell.out_point.index.trim_start_matches("0x"), 16).unwrap_or(0)
		})
		.await?;

	let data_hex = live
		.cell
		.and_then(|c| c.data)
		.map(|d| d.content)
		.unwrap_or_else(|| "0x".into());

	let data = hex::decode(data_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad identity cell data: {e}")))?;

	if data.len() < 50 {
		return Ok(None);
	}

	match decode_identity_data(&data) {
		Ok(id) => Ok(Some(id)),
		Err(_) => Ok(None),
	}
}

async fn find_worker_capability_nft(
	state: &AppState,
	worker_lock_args: &str,
	capability_hash: &[u8],
) -> Result<Value, TxBuildError> {
	let cap_nft_code_hash = std::env::var("CAP_NFT_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"CAP_NFT_TYPE_CODE_HASH not set; run scripts/deploy_contracts.sh first".into(),
		)
	})?;

	let type_script = Script {
		code_hash: cap_nft_code_hash,
		hash_type: "data1".into(),
		args: "0x".into(),
	};

	let cells = state.ckb.get_cells_by_type_script(&type_script, 200).await?;

	for cell in &cells.objects {
		let live = state
			.ckb
			.get_live_cell(&cell.out_point.tx_hash, {
				u32::from_str_radix(cell.out_point.index.trim_start_matches("0x"), 16).unwrap_or(0)
			})
			.await?;

		let data_hex = live
			.cell
			.and_then(|c| c.data)
			.map(|d| d.content)
			.unwrap_or_else(|| "0x".into());
		let data = hex::decode(data_hex.trim_start_matches("0x"))
			.map_err(|e| TxBuildError::Rpc(format!("bad capability NFT data: {e}")))?;

		// Capability NFT layout: [2..22] agent_lock_args, [22..54] capability_hash.
		if data.len() >= 54 {
			let nft_lock_args = &data[2..22];
			let nft_cap_hash = &data[22..54];

			let worker_bytes = parse_lock_args_20(worker_lock_args)?;
			if nft_lock_args == worker_bytes.as_slice() && nft_cap_hash == capability_hash {
				return Ok(json!(cell.out_point));
			}
		}
	}

	Err(TxBuildError::CellNotFound(format!(
		"no capability NFT found for worker {} matching capability_hash 0x{}",
		worker_lock_args,
		hex::encode(capability_hash)
	)))
}

/// When `result` text is provided, computes the binding hash blake2b(description_hash || result_data),
/// places the proof in witness input_type, and creates a result memo cell under the worker's lock.
/// Jobs with a non-zero description_hash require a result; description-less jobs settle without one.
pub async fn build_complete_job(
	state: &AppState,
	job_tx_hash: &str,
	job_index: u32,
	worker_lock_args: &str,
	result: Option<String>,
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
	let poster_refund = job_capacity.checked_sub(reward_shannons).ok_or_else(|| {
		TxBuildError::InsufficientCapacity {
			need: reward_shannons,
			have: job_capacity,
		}
	})?;

	if reward_shannons < MIN_PAYMENT_CELL {
		return Err(TxBuildError::InsufficientCapacity {
			need: MIN_PAYMENT_CELL,
			have: reward_shannons,
		});
	}

	let desc_hash = if job_data.len() >= 122 { &job_data[90..122] } else { &[0u8; 32][..] };
	let has_description = !desc_hash.iter().all(|&b| b == 0);

	if has_description && result.is_none() {
		return Err(TxBuildError::Rpc("result required for jobs with a description".into()));
	}

	let (result_hash, first_witness_hex) = if let Some(ref result_text) = result {
		let result_bytes = result_text.as_bytes();
		let mut preimage = Vec::with_capacity(32 + result_bytes.len());
		preimage.extend_from_slice(desc_hash);
		preimage.extend_from_slice(result_bytes);
		let hash = blake2b_256(&preimage);

		let mut proof = Vec::with_capacity(32 + result_bytes.len());
		proof.extend_from_slice(&hash);
		proof.extend_from_slice(result_bytes);

		let witness = placeholder_witness_with_input_type(&proof);
		(Some(hash), format!("0x{}", hex::encode(&witness)))
	} else {
		(None, format!("0x{}", hex::encode(placeholder_witness())))
	};

	let worker_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: worker_lock_args.into(),
	};

	let identity = fetch_worker_identity(state, worker_lock_args).await?;
	let (worker_amount, parent_share) = compute_revenue_split(reward_shannons, &identity);

	let memo_cost = if result_hash.is_some() { RESULT_MEMO_CAPACITY } else { 0 };
	let poster_refund_after_fee = poster_refund
		.checked_sub(ESTIMATED_FEE)
		.and_then(|v| v.checked_sub(memo_cost))
		.ok_or_else(|| TxBuildError::InsufficientCapacity {
			need: ESTIMATED_FEE + memo_cost,
			have: poster_refund,
		})?;

	let inputs = vec![json!({ "previous_output": { "tx_hash": job_tx_hash, "index": format!("{:#x}", job_index) }, "since": "0x0" })];

	let witnesses: Vec<Value> = vec![serde_json::Value::String(first_witness_hex)];

	let mut outputs = vec![
		json!({ "capacity": format!("{:#x}", worker_amount), "lock": worker_lock, "type": null }),
		json!({ "capacity": format!("{:#x}", poster_refund_after_fee), "lock": our_lock(state), "type": null }),
	];
	let mut outputs_data = vec!["0x".to_string(), "0x".to_string()];

	if let Some((parent_lock_args, parent_amount)) = &parent_share {
		let parent_lock = Script {
			code_hash: SECP256K1_CODE_HASH.into(),
			hash_type: SECP256K1_HASH_TYPE.into(),
			args: format!("0x{}", hex::encode(parent_lock_args)),
		};
		outputs.push(json!({
			"capacity": format!("{:#x}", parent_amount),
			"lock": parent_lock,
			"type": null,
		}));
		outputs_data.push("0x".to_string());
	}

	if let Some(hash) = result_hash {
		let memo_data = encode_result_memo(&hash);
		let memo_lock = Script {
			code_hash: SECP256K1_CODE_HASH.into(),
			hash_type: SECP256K1_HASH_TYPE.into(),
			args: worker_lock_args.into(),
		};
		outputs.push(json!({
			"capacity": format!("{:#x}", RESULT_MEMO_CAPACITY),
			"lock": memo_lock,
			"type": null,
		}));
		outputs_data.push(format!("0x{}", hex::encode(&memo_data)));
	}

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": outputs,
		"outputs_data": outputs_data,
		"witnesses": witnesses,
	});

	sign_and_finalize(state, tx).await
}

/// If either the parent's or worker's share would be below MIN_PAYMENT_CELL (61 CKB),
/// the worker gets 100% (best-effort split).
fn compute_revenue_split(
	reward_shannons: u64,
	identity: &Option<IdentityData>,
) -> (u64, Option<([u8; 20], u64)>) {
	let Some(id) = identity else {
		return (reward_shannons, None);
	};

	if id.parent_lock_args == [0u8; 20] {
		return (reward_shannons, None);
	}

	let bps = id.revenue_share_bps;
	if bps == 0 {
		return (reward_shannons, None);
	}

	let parent_amount = reward_shannons * bps as u64 / 10000;
	let worker_amount = reward_shannons - parent_amount;

	// Best-effort: if either share is below minimum cell capacity, give 100% to worker.
	if parent_amount < MIN_PAYMENT_CELL || worker_amount < MIN_PAYMENT_CELL {
		return (reward_shannons, None);
	}

	(worker_amount, Some((id.parent_lock_args, parent_amount)))
}

/// For non-Open jobs, includes a header_dep so the type script can verify TTL
/// has elapsed before allowing cancellation.
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

	// Include header_dep for non-Open cancellations (TTL enforcement in type script).
	let header_deps = if status != STATUS_OPEN {
		let tip_header_hash = state.ckb.get_tip_header_hash().await?;
		json!([tip_header_hash])
	} else {
		json!([])
	};

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": header_deps,
		"inputs": inputs,
		"outputs": [
			{ "capacity": format!("{:#x}", recovery), "lock": our_lock(state), "type": null },
		],
		"outputs_data": ["0x"],
		"witnesses": witnesses,
	});

	sign_and_finalize(state, tx).await
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encode_job_data_layout_no_description() {
		let poster = [0xAA; 20];
		let worker = [0x00; 20];
		let cap_hash = [0xCC; 32];
		let data = encode_job_data(&poster, &worker, 500_000_000, 1000, &cap_hash, &[0u8; 32], &[]);
		assert_eq!(data.len(), 122);
		assert_eq!(data[0], 0, "version");
		assert_eq!(data[1], STATUS_OPEN, "status");
		assert_eq!(&data[2..22], &poster);
		assert_eq!(&data[22..42], &worker);
		let reward = u64::from_le_bytes(data[42..50].try_into().unwrap());
		assert_eq!(reward, 500_000_000);
		let ttl = u64::from_le_bytes(data[50..58].try_into().unwrap());
		assert_eq!(ttl, 1000);
		assert_eq!(&data[58..90], &cap_hash);
		assert_eq!(&data[90..122], &[0u8; 32]);
	}

	#[test]
	fn encode_job_data_layout_with_description() {
		let poster = [0xAA; 20];
		let worker = [0x00; 20];
		let cap_hash = [0xCC; 32];
		let desc = b"Rent TikTok ad space";
		let desc_hash = blake2b_256(desc);
		let data = encode_job_data(&poster, &worker, 500_000_000, 1000, &cap_hash, &desc_hash, desc);
		assert_eq!(data.len(), 122 + desc.len());
		assert_eq!(&data[90..122], &desc_hash);
		assert_eq!(&data[122..], desc);
	}

	#[test]
	fn parse_lock_args_20_valid() {
		let hex = "0x".to_owned() + &"ab".repeat(20);
		let result = parse_lock_args_20(&hex).unwrap();
		assert_eq!(result, [0xAB; 20]);
	}

	#[test]
	fn parse_lock_args_20_wrong_length() {
		let short = "0xaabb";
		assert!(parse_lock_args_20(short).is_err());
	}

	#[test]
	fn parse_hash_32_valid() {
		let hex = "0x".to_owned() + &"ff".repeat(32);
		let result = parse_hash_32(&hex).unwrap();
		assert_eq!(result, [0xFF; 32]);
	}

	#[test]
	fn parse_hash_32_wrong_length() {
		let short = "0x".to_owned() + &"ff".repeat(16);
		assert!(parse_hash_32(&short).is_err());
	}

	#[test]
	fn encode_result_memo_layout() {
		let hash = [0xDD; 32];
		let data = encode_result_memo(&hash);
		assert_eq!(data.len(), 33);
		assert_eq!(data[0], 0, "version");
		assert_eq!(&data[1..33], &hash);
	}

	#[test]
	fn revenue_split_no_identity() {
		let reward = 200 * 100_000_000u64; // 200 CKB
		let (worker, parent) = compute_revenue_split(reward, &None);
		assert_eq!(worker, reward);
		assert!(parent.is_none());
	}

	#[test]
	fn revenue_split_root_agent() {
		let reward = 200 * 100_000_000u64;
		let id = IdentityData {
			pubkey: [0xAA; 33],
			spending_limit_shannons: 100_000_000,
			daily_limit_shannons: 500_000_000,
			parent_lock_args: [0u8; 20],
			revenue_share_bps: 0,
			daily_spent: 0,
			last_reset_epoch: 0,
		};
		let (worker, parent) = compute_revenue_split(reward, &Some(id));
		assert_eq!(worker, reward);
		assert!(parent.is_none());
	}

	#[test]
	fn revenue_split_with_parent() {
		let reward = 1000 * 100_000_000u64;
		let parent_args = [0xCC; 20];
		let id = IdentityData {
			pubkey: [0xBB; 33],
			spending_limit_shannons: 100_000_000,
			daily_limit_shannons: 500_000_000,
			parent_lock_args: parent_args,
			revenue_share_bps: 1000, // 10%
			daily_spent: 0,
			last_reset_epoch: 0,
		};
		let (worker, parent) = compute_revenue_split(reward, &Some(id));
		assert_eq!(worker, 900 * 100_000_000);
		let (p_args, p_amount) = parent.unwrap();
		assert_eq!(p_args, parent_args);
		assert_eq!(p_amount, 100 * 100_000_000);
	}

	#[test]
	fn revenue_split_zero_parent_is_root() {
		let reward = 200 * 100_000_000u64;
		let id = IdentityData {
			pubkey: [0xBB; 33],
			spending_limit_shannons: 100_000_000,
			daily_limit_shannons: 500_000_000,
			parent_lock_args: [0u8; 20],
			revenue_share_bps: 1000,
			daily_spent: 0,
			last_reset_epoch: 0,
		};
		let (worker, parent) = compute_revenue_split(reward, &Some(id));
		assert_eq!(worker, reward);
		assert!(parent.is_none());
	}

	#[test]
	fn revenue_split_below_min_cell_gives_worker_100pct() {
		let reward = 70 * 100_000_000u64;
		let id = IdentityData {
			pubkey: [0xBB; 33],
			spending_limit_shannons: 100_000_000,
			daily_limit_shannons: 500_000_000,
			parent_lock_args: [0xCC; 20],
			revenue_share_bps: 1000, // 10%
			daily_spent: 0,
			last_reset_epoch: 0,
		};
		let (worker, parent) = compute_revenue_split(reward, &Some(id));
		assert_eq!(worker, reward);
		assert!(parent.is_none());
	}

	#[test]
	fn revenue_split_zero_bps() {
		let reward = 200 * 100_000_000u64;
		let id = IdentityData {
			pubkey: [0xBB; 33],
			spending_limit_shannons: 100_000_000,
			daily_limit_shannons: 500_000_000,
			parent_lock_args: [0xCC; 20],
			revenue_share_bps: 0,
			daily_spent: 0,
			last_reset_epoch: 0,
		};
		let (worker, parent) = compute_revenue_split(reward, &Some(id));
		assert_eq!(worker, reward);
		assert!(parent.is_none());
	}
}
