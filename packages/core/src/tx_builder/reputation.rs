use serde_json::{json, Value};

use crate::{
	errors::TxBuildError,
	state::{parse_capacity_hex, AppState, SECP256K1_DEP_TX_HASH},
};

use super::{
	gather_fee_inputs, our_lock, placeholder_witnesses, MIN_CELL_CAPACITY,
	identity::calculate_type_id,
	molecule::compute_raw_tx_hash,
	signing::inject_witness,
};

const ESTIMATED_FEE: u64 = 2_000_000;
const REP_DATA_SIZE: usize = 110;
// Capacity: cap(8) + lock(53) + type(65) + data(110) = 236 bytes → 236 CKB.
const REP_CELL_CAPACITY: u64 = 236 * 100_000_000;

fn rep_type_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("REPUTATION_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"REPUTATION_TYPE_CODE_HASH not set; run scripts/deploy_contracts.sh reputation first"
				.into(),
		)
	})?;
	let dep_tx_hash = std::env::var("REPUTATION_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"REPUTATION_DEP_TX_HASH not set; run scripts/deploy_contracts.sh reputation first"
				.into(),
		)
	})?;
	Ok((code_hash, dep_tx_hash))
}

/// Layout (110 bytes):
///   [0]       version = 0
///   [1]       pending_type: 0=idle, 1=propose_completed, 2=propose_abandoned
///   [2..10]   jobs_completed: u64 LE
///   [10..18]  jobs_abandoned: u64 LE
///   [18..26]  pending_expires_at: u64 LE
///   [26..46]  agent_lock_args: [u8; 20]
///   [46..78]  proof_root: [u8; 32]
///   [78..110] pending_settlement_hash: [u8; 32]
fn encode_rep_data(
	pending_type: u8,
	jobs_completed: u64,
	jobs_abandoned: u64,
	pending_expires_at: u64,
	agent_lock_args: &[u8; 20],
	proof_root: &[u8; 32],
	settlement_hash: &[u8; 32],
) -> Vec<u8> {
	let mut data = Vec::with_capacity(REP_DATA_SIZE);
	data.push(0u8);
	data.push(pending_type);
	data.extend_from_slice(&jobs_completed.to_le_bytes());
	data.extend_from_slice(&jobs_abandoned.to_le_bytes());
	data.extend_from_slice(&pending_expires_at.to_le_bytes());
	data.extend_from_slice(agent_lock_args);
	data.extend_from_slice(proof_root);
	data.extend_from_slice(settlement_hash);
	data
}

fn parse_rep_data(
	data: &[u8],
) -> Result<(u8, u64, u64, u64, [u8; 20], [u8; 32], [u8; 32]), TxBuildError> {
	if data.len() < REP_DATA_SIZE {
		return Err(TxBuildError::Rpc(format!(
			"reputation cell data too short: {} bytes, need {}",
			data.len(),
			REP_DATA_SIZE,
		)));
	}
	let pending_type = data[1];
	let jobs_completed = u64::from_le_bytes(data[2..10].try_into().unwrap());
	let jobs_abandoned = u64::from_le_bytes(data[10..18].try_into().unwrap());
	let pending_expires_at = u64::from_le_bytes(data[18..26].try_into().unwrap());
	let mut agent_lock_args = [0u8; 20];
	agent_lock_args.copy_from_slice(&data[26..46]);
	let mut proof_root = [0u8; 32];
	proof_root.copy_from_slice(&data[46..78]);
	let mut settlement_hash = [0u8; 32];
	settlement_hash.copy_from_slice(&data[78..110]);
	Ok((pending_type, jobs_completed, jobs_abandoned, pending_expires_at, agent_lock_args, proof_root, settlement_hash))
}

pub fn compute_settlement_hash(
	job_tx_hash: &[u8; 32],
	job_index: u32,
	worker_lock_args: &[u8; 20],
	poster_lock_args: &[u8; 20],
	reward_shannons: u64,
	result_hash: Option<&[u8; 32]>,
) -> [u8; 32] {
	use blake2b_rs::Blake2bBuilder;

	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(job_tx_hash);
	hasher.update(&job_index.to_le_bytes());
	hasher.update(worker_lock_args);
	hasher.update(poster_lock_args);
	hasher.update(&reward_shannons.to_le_bytes());
	if let Some(rh) = result_hash {
		hasher.update(rh);
	}
	let mut out = [0u8; 32];
	hasher.finalize(&mut out);
	out
}

pub fn compute_proof_root(old_root: &[u8; 32], settlement_hash: &[u8; 32]) -> [u8; 32] {
	use blake2b_rs::Blake2bBuilder;

	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(old_root);
	hasher.update(settlement_hash);
	let mut out = [0u8; 32];
	hasher.finalize(&mut out);
	out
}

async fn fetch_rep_cell(
	state: &AppState,
	tx_hash: &str,
	index: u32,
) -> Result<(u64, Vec<u8>, String), TxBuildError> {
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

	let type_args = cell.output.type_script
		.as_ref()
		.map(|ts| ts.args.clone())
		.unwrap_or_else(|| "0x".into());

	Ok((capacity, data, type_args))
}

pub async fn build_create_reputation(
	state: &AppState,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = rep_type_env()?;
	let agent_lock_args = super::job::parse_lock_args_20(&state.lock_args)?;

	let rep_data = encode_rep_data(0, 0, 0, 0, &agent_lock_args, &[0u8; 32], &[0u8; 32]);

	let needed = REP_CELL_CAPACITY + ESTIMATED_FEE + MIN_CELL_CAPACITY;
	let agent_lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&agent_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	let mut first_input_tx_hash: Option<String> = None;
	let mut first_input_index: u32 = 0;
	for cell in &cells.objects {
		if cell.output.type_script.is_some() {
			continue;
		}
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		if first_input_tx_hash.is_none() {
			first_input_tx_hash = Some(cell.out_point.tx_hash.clone());
			first_input_index = u32::from_str_radix(
				cell.out_point.index.trim_start_matches("0x"),
				16,
			)
			.unwrap_or(0);
		}
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

	let first_tx_hash = first_input_tx_hash
		.ok_or_else(|| TxBuildError::Rpc("no input cells available for type_id".into()))?;

	let type_id_args = calculate_type_id(&first_tx_hash, first_input_index, 0, 0)?;

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
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": type_id_args },
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

	let tx_hash_str = compute_raw_tx_hash(&tx)?;
	let signature = state.signer.sign(&tx_hash_str, inputs.len()).await?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash_str))
}

pub async fn build_propose_reputation(
	state: &AppState,
	rep_tx_hash: &str,
	rep_index: u32,
	propose_type: u8,
	dispute_window_blocks: u64,
	settlement_hash: [u8; 32],
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = rep_type_env()?;

	if propose_type != 1 && propose_type != 2 {
		return Err(TxBuildError::Rpc(
			"propose_type must be 1 (completed) or 2 (abandoned)".into(),
		));
	}

	let (rep_capacity, rep_data_bytes, type_args) =
		fetch_rep_cell(state, rep_tx_hash, rep_index).await?;
	let (pending_type, jobs_completed, jobs_abandoned, _, agent_lock_args, proof_root, _) =
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
		&proof_root,
		&settlement_hash,
	);

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
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": type_args },
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

	let tx_hash_str = compute_raw_tx_hash(&tx)?;
	let signature = state.signer.sign(&tx_hash_str, all_inputs.len()).await?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash_str))
}

pub async fn build_finalize_reputation(
	state: &AppState,
	rep_tx_hash: &str,
	rep_index: u32,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = rep_type_env()?;

	let (rep_capacity, rep_data_bytes, type_args) =
		fetch_rep_cell(state, rep_tx_hash, rep_index).await?;
	let (pending_type, jobs_completed, jobs_abandoned, pending_expires_at, agent_lock_args, proof_root, settlement_hash) =
		parse_rep_data(&rep_data_bytes)?;

	if pending_type == 0 {
		return Err(TxBuildError::Rpc("reputation cell has no pending proposal".into()));
	}

	let (new_completed, new_abandoned) = match pending_type {
		1 => (jobs_completed + 1, jobs_abandoned),
		2 => (jobs_completed, jobs_abandoned + 1),
		_ => return Err(TxBuildError::Rpc(format!("unknown pending_type: {pending_type}"))),
	};

	let new_proof_root = compute_proof_root(&proof_root, &settlement_hash);
	let new_data = encode_rep_data(0, new_completed, new_abandoned, 0, &agent_lock_args, &new_proof_root, &[0u8; 32]);

	let (fee_inputs, fee_capacity) = gather_fee_inputs(state, ESTIMATED_FEE).await?;
	let change_capacity = fee_capacity - ESTIMATED_FEE;

	// CKB `since` prevents inclusion before the pending period expires.
	let since_value = format!("{:#x}", pending_expires_at);

	let mut all_inputs = vec![json!({
		"previous_output": { "tx_hash": rep_tx_hash, "index": format!("{:#x}", rep_index) },
		"since": since_value,
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
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": type_args },
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

	let tx_hash_str = compute_raw_tx_hash(&tx)?;
	let signature = state.signer.sign(&tx_hash_str, all_inputs.len()).await?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash_str))
}


#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encode_rep_data_layout() {
		let agent = [0xBB; 20];
		let proof_root = [0xAA; 32];
		let settlement = [0x11; 32];
		let data = encode_rep_data(1, 10, 3, 999, &agent, &proof_root, &settlement);
		assert_eq!(data.len(), REP_DATA_SIZE);
		assert_eq!(data[0], 0, "version");
		assert_eq!(data[1], 1, "pending_type");
		let completed = u64::from_le_bytes(data[2..10].try_into().unwrap());
		assert_eq!(completed, 10);
		let abandoned = u64::from_le_bytes(data[10..18].try_into().unwrap());
		assert_eq!(abandoned, 3);
		let expires = u64::from_le_bytes(data[18..26].try_into().unwrap());
		assert_eq!(expires, 999);
		assert_eq!(&data[26..46], &agent);
		assert_eq!(&data[46..78], &proof_root);
		assert_eq!(&data[78..110], &settlement);
	}

	#[test]
	fn encode_parse_roundtrip() {
		let agent = [0xCC; 20];
		let proof_root = [0xDD; 32];
		let settlement = [0xEE; 32];
		let data = encode_rep_data(2, 42, 7, 12345, &agent, &proof_root, &settlement);
		let (pt, c, a, e, la, pr, sh) = parse_rep_data(&data).unwrap();
		assert_eq!(pt, 2);
		assert_eq!(c, 42);
		assert_eq!(a, 7);
		assert_eq!(e, 12345);
		assert_eq!(la, agent);
		assert_eq!(pr, proof_root);
		assert_eq!(sh, settlement);
	}

	#[test]
	fn parse_rep_data_rejects_short() {
		let short = vec![0u8; 10];
		assert!(parse_rep_data(&short).is_err());
	}

	#[test]
	fn compute_settlement_hash_deterministic() {
		let job_tx = [0x11; 32];
		let worker = [0x22; 20];
		let poster = [0x33; 20];
		let result = [0x44; 32];

		let h1 = compute_settlement_hash(&job_tx, 0, &worker, &poster, 500_000_000, Some(&result));
		let h2 = compute_settlement_hash(&job_tx, 0, &worker, &poster, 500_000_000, Some(&result));
		assert_eq!(h1, h2, "same inputs must produce same hash");

		let h3 = compute_settlement_hash(&job_tx, 1, &worker, &poster, 500_000_000, Some(&result));
		assert_ne!(h1, h3, "different index must produce different hash");
	}

	#[test]
	fn compute_proof_root_chain() {
		let genesis = [0u8; 32];
		let settlement_1 = [0x11; 32];
		let settlement_2 = [0x22; 32];

		// Genesis → 1 job.
		let root_1 = compute_proof_root(&genesis, &settlement_1);
		assert_ne!(root_1, genesis, "proof root must change after first job");

		// 1 job → 2 jobs.
		let root_2 = compute_proof_root(&root_1, &settlement_2);
		assert_ne!(root_2, root_1, "proof root must change after second job");
		assert_ne!(root_2, genesis, "proof root must differ from genesis");

		// Verify determinism: replaying the same chain produces the same root.
		let replay_1 = compute_proof_root(&genesis, &settlement_1);
		let replay_2 = compute_proof_root(&replay_1, &settlement_2);
		assert_eq!(replay_2, root_2, "replaying the chain must produce the same root");
	}
}
