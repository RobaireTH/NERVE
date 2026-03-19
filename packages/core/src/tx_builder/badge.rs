use blake2b_rs::Blake2bBuilder;
use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{parse_capacity_hex, AppState, SECP256K1_CODE_HASH, SECP256K1_DEP_TX_HASH, SECP256K1_HASH_TYPE},
};

use super::identity::calculate_type_id;
use super::molecule::compute_raw_tx_hash;
use super::signing::{inject_witness, sign_tx};

const ESTIMATED_FEE: u64 = 2_000_000;
// Minimum capacity for a badge cell:
//   cap(8) + lock(53) + type(33 + 60 args) + data(34) = 188 bytes → 188 CKB.
const BADGE_CELL_CAPACITY: u64 = 188 * 100_000_000;

fn dob_badge_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("DOB_BADGE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"DOB_BADGE_CODE_HASH not set; deploy or configure the dob-badge contract first".into(),
		)
	})?;
	let dep_tx_hash = std::env::var("DOB_BADGE_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"DOB_BADGE_DEP_TX_HASH not set; deploy or configure the dob-badge contract first"
				.into(),
		)
	})?;
	Ok((code_hash, dep_tx_hash))
}

use super::{our_lock, placeholder_witnesses, MIN_CELL_CAPACITY};

/// Computes the event_id_hash from a job's outpoint: blake2b(tx_hash || index_u64_le)[..20].
pub fn compute_event_id_hash(job_tx_hash: &[u8; 32], job_index: u64) -> [u8; 20] {
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(job_tx_hash);
	hasher.update(&job_index.to_le_bytes());
	let mut full = [0u8; 32];
	hasher.finalize(&mut full);
	let mut out = [0u8; 20];
	out.copy_from_slice(&full[..20]);
	out
}

/// Computes the recipient_hash from lock_args: blake2b(lock_args_bytes)[..20].
pub fn compute_recipient_hash(lock_args: &[u8; 20]) -> [u8; 20] {
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(lock_args);
	let mut full = [0u8; 32];
	hasher.finalize(&mut full);
	let mut out = [0u8; 20];
	out.copy_from_slice(&full[..20]);
	out
}

/// Encodes badge cell data: [0x01, 0x01, blake2b(content_json)[0..32]] = 34 bytes.
pub fn encode_badge_data(content_json: &str) -> [u8; 34] {
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(content_json.as_bytes());
	let mut hash = [0u8; 32];
	hasher.finalize(&mut hash);

	let mut data = [0u8; 34];
	data[0] = 0x01;
	data[1] = 0x01;
	data[2..34].copy_from_slice(&hash);
	data
}

/// 60-byte type_args: type_id[20] || event_id_hash[20] || recipient_hash[20].
/// The badge cell is placed under the worker's lock, making it soulbound.
pub async fn build_mint_badge(
	state: &AppState,
	job_tx_hash: &str,
	job_index: u32,
	worker_lock_args: &str,
	result_hash: Option<&str>,
	completed_at_tx: &str,
) -> Result<(Value, String), TxBuildError> {
	let (badge_code_hash, badge_dep_tx_hash) = dob_badge_env()?;

	let job_tx_bytes = hex::decode(job_tx_hash.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::InvalidTypeArgs(format!("bad job_tx_hash: {e}")))?;
	if job_tx_bytes.len() != 32 {
		return Err(TxBuildError::InvalidTypeArgs("job_tx_hash must be 32 bytes".into()));
	}
	let mut job_hash = [0u8; 32];
	job_hash.copy_from_slice(&job_tx_bytes);

	let worker_args = super::job::parse_lock_args_20(worker_lock_args)?;

	let event_id_hash = compute_event_id_hash(&job_hash, job_index as u64);
	let recipient_hash = compute_recipient_hash(&worker_args);

	let content_json = format!(
		r#"{{"protocol":"nerve","version":1,"job_tx_hash":"{}","job_index":{},"worker_lock_args":"{}","result_hash":"{}","completed_at_tx":"{}"}}"#,
		job_tx_hash,
		job_index,
		worker_lock_args,
		result_hash.unwrap_or("null"),
		completed_at_tx,
	);
	let badge_data = encode_badge_data(&content_json);

	let worker_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: worker_lock_args.into(),
	};

	let needed = BADGE_CELL_CAPACITY + ESTIMATED_FEE + MIN_CELL_CAPACITY;
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

	let type_id_full = calculate_type_id(&first_tx_hash, first_input_index, 0, 0)?;
	let type_id_bytes = hex::decode(type_id_full.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad type_id: {e}")))?;

	// Build 60-byte type args: type_id[20] || event_id_hash[20] || recipient_hash[20].
	let mut type_args = Vec::with_capacity(60);
	type_args.extend_from_slice(&type_id_bytes[..20]);
	type_args.extend_from_slice(&event_id_hash);
	type_args.extend_from_slice(&recipient_hash);

	let change_capacity = input_capacity - BADGE_CELL_CAPACITY - ESTIMATED_FEE;
	let witnesses = placeholder_witnesses(inputs.len());

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": badge_dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", BADGE_CELL_CAPACITY),
				"lock": worker_lock,
				"type": {
					"code_hash": badge_code_hash,
					"hash_type": "type",
					"args": format!("0x{}", hex::encode(&type_args)),
				},
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(badge_data)), "0x"],
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;
	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn compute_event_id_hash_deterministic() {
		let tx_hash = [0xAA; 32];
		let h1 = compute_event_id_hash(&tx_hash, 0);
		let h2 = compute_event_id_hash(&tx_hash, 0);
		assert_eq!(h1, h2);
		assert_eq!(h1.len(), 20);
	}

	#[test]
	fn compute_event_id_hash_varies_with_index() {
		let tx_hash = [0xAA; 32];
		let h0 = compute_event_id_hash(&tx_hash, 0);
		let h1 = compute_event_id_hash(&tx_hash, 1);
		assert_ne!(h0, h1);
	}

	#[test]
	fn compute_recipient_hash_deterministic() {
		let args = [0xBB; 20];
		let h1 = compute_recipient_hash(&args);
		let h2 = compute_recipient_hash(&args);
		assert_eq!(h1, h2);
		assert_eq!(h1.len(), 20);
	}

	#[test]
	fn encode_badge_data_layout() {
		let data = encode_badge_data(r#"{"protocol":"nerve"}"#);
		assert_eq!(data.len(), 34);
		assert_eq!(data[0], 0x01, "protocol version");
		assert_eq!(data[1], 0x01, "content type");
		// Content hash should be non-zero.
		assert!(!data[2..34].iter().all(|&b| b == 0));
	}

	#[test]
	fn encode_badge_data_deterministic() {
		let d1 = encode_badge_data("test");
		let d2 = encode_badge_data("test");
		assert_eq!(d1, d2);
	}
}
