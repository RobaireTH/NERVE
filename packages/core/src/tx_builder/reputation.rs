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

const ESTIMATED_FEE: u64 = 2_000_000;
const MIN_CELL_CAPACITY: u64 = 61 * 100_000_000;
const REP_DATA_SIZE: usize = 46;
// Minimum capacity for a reputation cell:
//   cap(8) + lock(53) + type(33) + data(46) = 140 bytes → 140 CKB.
const REP_CELL_CAPACITY: u64 = 140 * 100_000_000;

fn rep_type_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("REPUTATION_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"REPUTATION_TYPE_CODE_HASH not set — run scripts/deploy_contracts.sh reputation first"
				.into(),
		)
	})?;
	let dep_tx_hash = std::env::var("REPUTATION_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"REPUTATION_DEP_TX_HASH not set — run scripts/deploy_contracts.sh reputation first"
				.into(),
		)
	})?;
	Ok((code_hash, dep_tx_hash))
}

fn our_lock(state: &AppState) -> Script {
	Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	}
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

/// Encode reputation cell data (46 bytes).
///
/// Layout:
///   [0]       version = 0
///   [1]       pending_type: 0=idle, 1=propose_completed, 2=propose_abandoned
///   [2..10]   jobs_completed: u64 LE
///   [10..18]  jobs_abandoned: u64 LE
///   [18..26]  pending_expires_at: u64 LE
///   [26..46]  agent_lock_args: [u8; 20]
fn encode_rep_data(
	pending_type: u8,
	jobs_completed: u64,
	jobs_abandoned: u64,
	pending_expires_at: u64,
	agent_lock_args: &[u8; 20],
) -> Vec<u8> {
	let mut data = Vec::with_capacity(REP_DATA_SIZE);
	data.push(0u8); // version
	data.push(pending_type);
	data.extend_from_slice(&jobs_completed.to_le_bytes());
	data.extend_from_slice(&jobs_abandoned.to_le_bytes());
	data.extend_from_slice(&pending_expires_at.to_le_bytes());
	data.extend_from_slice(agent_lock_args);
	data
}

fn parse_rep_data(data: &[u8]) -> Result<(u8, u64, u64, u64, [u8; 20]), TxBuildError> {
	if data.len() < REP_DATA_SIZE {
		return Err(TxBuildError::Rpc("reputation cell data too short".into()));
	}
	let pending_type = data[1];
	let jobs_completed = u64::from_le_bytes(data[2..10].try_into().unwrap());
	let jobs_abandoned = u64::from_le_bytes(data[10..18].try_into().unwrap());
	let pending_expires_at = u64::from_le_bytes(data[18..26].try_into().unwrap());
	let mut agent_lock_args = [0u8; 20];
	agent_lock_args.copy_from_slice(&data[26..46]);
	Ok((pending_type, jobs_completed, jobs_abandoned, pending_expires_at, agent_lock_args))
}

/// Fetches a live reputation cell by outpoint.
async fn fetch_rep_cell(
	state: &AppState,
	tx_hash: &str,
	index: u32,
) -> Result<(u64, Vec<u8>), TxBuildError> {
	let result = state.ckb.get_live_cell(tx_hash, index).await?;
	if result.status != "live" {
		return Err(TxBuildError::CellNotFound(format!(
			"rep {}:{} status={}",
			tx_hash, index, result.status
		)));
	}
	let cell = result
		.cell
		.ok_or_else(|| TxBuildError::CellNotFound(format!("{tx_hash}:{index}")))?;
	let capacity = parse_capacity_hex(&cell.output.capacity)?;
	let data_hex = cell.data.map(|d| d.content).unwrap_or_else(|| "0x".into());
	let data = hex::decode(data_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad rep data: {e}")))?;
	Ok((capacity, data))
}

/// Creates a new reputation cell for an agent (initial state: Idle, zero counters).
pub async fn build_create_reputation(
	state: &AppState,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = rep_type_env()?;
	let agent_lock_args = super::job::parse_lock_args_20(&state.lock_args)?;

	let rep_data = encode_rep_data(0, 0, 0, 0, &agent_lock_args);

	let needed = REP_CELL_CAPACITY + ESTIMATED_FEE + MIN_CELL_CAPACITY;
	let agent_lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&agent_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	for cell in &cells.objects {
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		inputs.push(json!({ "previous_output": cell.out_point, "since": "0x0" }));
		input_capacity += cap;
		if input_capacity >= needed {
			break;
		}
	}
	if input_capacity < needed {
		return Err(TxBuildError::InsufficientFunds {
			need: needed as f64 / 1e8,
			have: input_capacity as f64 / 1e8,
		});
	}

	let change_capacity = input_capacity - REP_CELL_CAPACITY - ESTIMATED_FEE;
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
			{
				"capacity": format!("{:#x}", REP_CELL_CAPACITY),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(&rep_data)), "0x"],
		"witnesses": witnesses,
	});

	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash_str = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();
	let signature = sign_tx(&tx_hash_str, &state.private_key)?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash_str))
}

/// Proposes a reputation update (Idle → Proposed).
///
/// `propose_type`: 1 = completed, 2 = abandoned.
/// `dispute_window_blocks`: number of blocks until the proposal can be finalized.
pub async fn build_propose_reputation(
	state: &AppState,
	rep_tx_hash: &str,
	rep_index: u32,
	propose_type: u8,
	dispute_window_blocks: u64,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = rep_type_env()?;

	if propose_type != 1 && propose_type != 2 {
		return Err(TxBuildError::Rpc(
			"propose_type must be 1 (completed) or 2 (abandoned)".into(),
		));
	}

	let (rep_capacity, rep_data_bytes) = fetch_rep_cell(state, rep_tx_hash, rep_index).await?;
	let (pending_type, jobs_completed, jobs_abandoned, _, agent_lock_args) =
		parse_rep_data(&rep_data_bytes)?;

	if pending_type != 0 {
		return Err(TxBuildError::Rpc(format!(
			"reputation cell pending_type is {pending_type}, expected 0 (Idle)"
		)));
	}

	let tip = state.ckb.get_tip_block_number().await?;
	let expires_at = tip + dispute_window_blocks;

	let new_data = encode_rep_data(
		propose_type,
		jobs_completed,
		jobs_abandoned,
		expires_at,
		&agent_lock_args,
	);

	// Fee inputs.
	let (fee_inputs, fee_capacity) = gather_fee_inputs(state, ESTIMATED_FEE).await?;
	let change_capacity = fee_capacity - ESTIMATED_FEE;

	let mut all_inputs = vec![json!({
		"previous_output": { "tx_hash": rep_tx_hash, "index": format!("{:#x}", rep_index) },
		"since": "0x0",
	})];
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
				"capacity": format!("{:#x}", rep_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(&new_data)), "0x"],
		"witnesses": witnesses,
	});

	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash_str = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();
	let signature = sign_tx(&tx_hash_str, &state.private_key)?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash_str))
}

/// Finalizes a proposed reputation update (Proposed → Finalized).
///
/// Increments the relevant counter and clears the pending state.
pub async fn build_finalize_reputation(
	state: &AppState,
	rep_tx_hash: &str,
	rep_index: u32,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = rep_type_env()?;

	let (rep_capacity, rep_data_bytes) = fetch_rep_cell(state, rep_tx_hash, rep_index).await?;
	let (pending_type, jobs_completed, jobs_abandoned, _, agent_lock_args) =
		parse_rep_data(&rep_data_bytes)?;

	if pending_type == 0 {
		return Err(TxBuildError::Rpc("reputation cell has no pending proposal".into()));
	}

	let (new_completed, new_abandoned) = match pending_type {
		1 => (jobs_completed + 1, jobs_abandoned),
		2 => (jobs_completed, jobs_abandoned + 1),
		_ => return Err(TxBuildError::Rpc(format!("unknown pending_type: {pending_type}"))),
	};

	// Finalized: pending_type=0, pending_expires_at=0.
	let new_data = encode_rep_data(0, new_completed, new_abandoned, 0, &agent_lock_args);

	let (fee_inputs, fee_capacity) = gather_fee_inputs(state, ESTIMATED_FEE).await?;
	let change_capacity = fee_capacity - ESTIMATED_FEE;

	let mut all_inputs = vec![json!({
		"previous_output": { "tx_hash": rep_tx_hash, "index": format!("{:#x}", rep_index) },
		"since": "0x0",
	})];
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
				"capacity": format!("{:#x}", rep_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(&new_data)), "0x"],
		"witnesses": witnesses,
	});

	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash_str = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();
	let signature = sign_tx(&tx_hash_str, &state.private_key)?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash_str))
}

/// Gathers enough of the agent's secp256k1 cells to cover `needed` shannons.
async fn gather_fee_inputs(
	state: &AppState,
	needed: u64,
) -> Result<(Vec<Value>, u64), TxBuildError> {
	let agent_lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&agent_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut capacity: u64 = 0;
	for cell in &cells.objects {
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		inputs.push(json!({ "previous_output": cell.out_point, "since": "0x0" }));
		capacity += cap;
		if capacity >= needed + MIN_CELL_CAPACITY {
			break;
		}
	}
	if capacity < needed + MIN_CELL_CAPACITY {
		return Err(TxBuildError::InsufficientFunds {
			need: (needed + MIN_CELL_CAPACITY) as f64 / 1e8,
			have: capacity as f64 / 1e8,
		});
	}
	Ok((inputs, capacity))
}
