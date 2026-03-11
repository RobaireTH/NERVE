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

// Job cell data layout constants.
const JOB_STATUS_OPEN: u8 = 0;

// Minimum capacity for a job cell (bytes): cap(8) + lock(53) + type(33) + data(90) = 184 bytes.
const JOB_CELL_OVERHEAD: u64 = 184 * 100_000_000; // shannons

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

/// Encodes a job cell's 90-byte data field.
pub fn encode_job_data(
	poster_lock_args: &[u8; 20],
	reward_shannons: u64,
	ttl_block_height: u64,
	capability_hash: &[u8; 32],
) -> Vec<u8> {
	let mut data = Vec::with_capacity(90);
	data.push(0u8);                          // version
	data.push(JOB_STATUS_OPEN);             // status: Open
	data.extend_from_slice(poster_lock_args); // poster_lock_args [20]
	data.extend_from_slice(&[0u8; 20]);      // worker_lock_args (empty)
	data.extend_from_slice(&reward_shannons.to_le_bytes());
	data.extend_from_slice(&ttl_block_height.to_le_bytes());
	data.extend_from_slice(capability_hash);
	data
}

/// Parses lock_args hex (0x-prefixed 20-byte) to [u8; 20].
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

/// Builds, signs, and returns a post-job transaction.
///
/// Creates a job cell with the given reward locked inside. The cell carries
/// the job state machine type script; the reward is released on completion.
pub async fn build_post_job(
	state: &AppState,
	reward_shannons: u64,
	ttl_blocks: u64,
	capability_hash: [u8; 32],
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = job_type_env()?;

	// Compute ttl_block_height = current_tip + ttl_blocks.
	let tip = state.ckb.get_tip_block_number().await?;
	let ttl_block_height = tip + ttl_blocks;

	// Decode the agent's lock_args to [u8; 20] for embedding in job data.
	let poster_lock_args = parse_lock_args_20(&state.lock_args)?;

	let job_data = encode_job_data(
		&poster_lock_args,
		reward_shannons,
		ttl_block_height,
		&capability_hash,
	);

	let job_cell_capacity = JOB_CELL_OVERHEAD + reward_shannons;
	let needed = job_cell_capacity + ESTIMATED_FEE;

	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	for cell in &cells.objects {
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		inputs.push(json!({ "previous_output": cell.out_point, "since": "0x0" }));
		input_capacity += cap;
		if input_capacity >= needed + JOB_CELL_OVERHEAD {
			// Need at least overhead for the change cell too.
			break;
		}
	}

	if input_capacity < needed + JOB_CELL_OVERHEAD {
		return Err(TxBuildError::InsufficientFunds {
			need: (needed + JOB_CELL_OVERHEAD) as f64 / 1e8,
			have: input_capacity as f64 / 1e8,
		});
	}

	let change_capacity = input_capacity - job_cell_capacity - ESTIMATED_FEE;

	let placeholder_hex = format!("0x{}", hex::encode(placeholder_witness()));
	let witnesses: Vec<Value> = inputs
		.iter()
		.enumerate()
		.map(|(i, _)| {
			if i == 0 {
				serde_json::Value::String(placeholder_hex.clone())
			} else {
				serde_json::Value::String("0x".into())
			}
		})
		.collect();

	let mut tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", job_cell_capacity),
				"lock": our_lock.clone(),
				"type": {
					"code_hash": type_code_hash,
					"hash_type": "data1",
					"args": "0x",
				},
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock,
				"type": null,
			}
		],
		"outputs_data": [
			format!("0x{}", hex::encode(&job_data)),
			"0x",
		],
		"witnesses": witnesses,
	});

	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();

	let signature = sign_tx(&tx_hash, &state.private_key)?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}
