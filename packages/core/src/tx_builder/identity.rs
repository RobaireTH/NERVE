use blake2b_rs::Blake2bBuilder;
use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{
		parse_capacity_hex, AppState, SECP256K1_CODE_HASH, SECP256K1_DEP_TX_HASH,
		SECP256K1_HASH_TYPE,
	},
};

use super::molecule::compute_raw_tx_hash;
use super::signing::{inject_witness, placeholder_witness};

// Agent identity cell data layout (88 bytes):
//   [0]      version = 0
//   [1..34]  compressed secp256k1 pubkey (33 bytes)
//   [34..42] spending_limit_per_tx as u64 LE
//   [42..50] daily_limit as u64 LE
//   [50..70] parent_lock_args (20 bytes, all zeros = root agent)
//   [70..72] revenue_share_bps (u16 LE, basis points: 1000 = 10%)
//   [72..80] daily_spent as u64 LE (accumulated spending in current day window)
//   [80..88] last_reset_epoch as u64 LE (epoch number when accumulator last reset)
const IDENTITY_DATA_SIZE: usize = 88;

// Minimum capacity shannons for an identity cell:
//   capacity(8) + lock(53) + type_script(33 + 32 args) + data(50) = 176 bytes
// Add buffer to round up to 232 CKB for safety.
const IDENTITY_CELL_CAPACITY: u64 = 232 * 100_000_000;

// Estimated fee for the spawn transaction (shannons).
const ESTIMATED_FEE: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct IdentityData {
	pub pubkey: [u8; 33],
	pub spending_limit_shannons: u64,
	pub daily_limit_shannons: u64,
	pub parent_lock_args: [u8; 20],
	pub revenue_share_bps: u16,
	pub daily_spent: u64,
	pub last_reset_epoch: u64,
}

pub fn encode_identity_data(
	pubkey: &[u8; 33],
	spending_limit_shannons: u64,
	daily_limit_shannons: u64,
	parent_lock_args: &[u8; 20],
	revenue_share_bps: u16,
	daily_spent: u64,
	last_reset_epoch: u64,
) -> Vec<u8> {
	let mut data = Vec::with_capacity(IDENTITY_DATA_SIZE);
	data.push(0u8);
	data.extend_from_slice(pubkey);
	data.extend_from_slice(&spending_limit_shannons.to_le_bytes());
	data.extend_from_slice(&daily_limit_shannons.to_le_bytes());
	data.extend_from_slice(parent_lock_args);
	data.extend_from_slice(&revenue_share_bps.to_le_bytes());
	data.extend_from_slice(&daily_spent.to_le_bytes());
	data.extend_from_slice(&last_reset_epoch.to_le_bytes());
	data
}

pub fn decode_identity_data(data: &[u8]) -> Result<IdentityData, TxBuildError> {
	if data.len() < IDENTITY_DATA_SIZE {
		return Err(TxBuildError::Rpc(format!(
			"identity data too short: {} bytes, need {}",
			data.len(),
			IDENTITY_DATA_SIZE
		)));
	}

	if data[0] != 0 {
		return Err(TxBuildError::Rpc(format!("unknown identity version: {}", data[0])));
	}

	let mut pubkey = [0u8; 33];
	pubkey.copy_from_slice(&data[1..34]);
	let spending_limit_shannons = u64::from_le_bytes(data[34..42].try_into().unwrap());
	let daily_limit_shannons = u64::from_le_bytes(data[42..50].try_into().unwrap());
	let mut parent_lock_args = [0u8; 20];
	parent_lock_args.copy_from_slice(&data[50..70]);
	let revenue_share_bps = u16::from_le_bytes(data[70..72].try_into().unwrap());
	let daily_spent = u64::from_le_bytes(data[72..80].try_into().unwrap());
	let last_reset_epoch = u64::from_le_bytes(data[80..88].try_into().unwrap());

	Ok(IdentityData {
		pubkey,
		spending_limit_shannons,
		daily_limit_shannons,
		parent_lock_args,
		revenue_share_bps,
		daily_spent,
		last_reset_epoch,
	})
}

pub fn blake2b_256(data: &[u8]) -> [u8; 32] {
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(data);
	let mut out = [0u8; 32];
	hasher.finalize(&mut out);
	out
}

/// type_id = blake2b(since(8) || prev_tx_hash(32) || prev_index(4) || output_index(8))
pub fn calculate_type_id(
	first_input_tx_hash: &str,
	first_input_index: u32,
	first_input_since: u64,
	output_index: u64,
) -> Result<String, TxBuildError> {
	let tx_hash_bytes = hex::decode(first_input_tx_hash.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad tx_hash in type_id calc: {e}")))?;
	if tx_hash_bytes.len() != 32 {
		return Err(TxBuildError::Rpc("type_id: tx_hash must be 32 bytes".into()));
	}

	// Reconstruct CellInput molecule bytes: since(8) + tx_hash(32) + index(4).
	let mut cell_input = Vec::with_capacity(44);
	cell_input.extend_from_slice(&first_input_since.to_le_bytes());
	cell_input.extend_from_slice(&tx_hash_bytes);
	cell_input.extend_from_slice(&first_input_index.to_le_bytes());

	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(&cell_input);
	hasher.update(&output_index.to_le_bytes());

	let mut type_id = [0u8; 32];
	hasher.finalize(&mut type_id);
	Ok(format!("0x{}", hex::encode(type_id)))
}

/// Reads the type script code_hash from `AGENT_IDENTITY_TYPE_CODE_HASH` env var
/// and the dep tx_hash from `AGENT_IDENTITY_DEP_TX_HASH`. Both must be set after
/// the contract is deployed via `POST /admin/deploy-bin`.
pub async fn build_spawn_agent(
	state: &AppState,
	compressed_pubkey: &[u8; 33],
	spending_limit_shannons: u64,
	daily_limit_shannons: u64,
) -> Result<(Value, String), TxBuildError> {
	let type_code_hash = std::env::var("AGENT_IDENTITY_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"AGENT_IDENTITY_TYPE_CODE_HASH not set; run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	let dep_tx_hash = std::env::var("AGENT_IDENTITY_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"AGENT_IDENTITY_DEP_TX_HASH not set; run scripts/deploy_contracts.sh first".into(),
		)
	})?;

	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;
	let needed = IDENTITY_CELL_CAPACITY + ESTIMATED_FEE;

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
		inputs.push(json!({
			"previous_output": cell.out_point,
			"since": "0x0",
		}));
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

	let change_capacity = input_capacity - IDENTITY_CELL_CAPACITY - ESTIMATED_FEE;

	let identity_data = encode_identity_data(
		compressed_pubkey,
		spending_limit_shannons,
		daily_limit_shannons,
		&[0u8; 20],
		0,
		0,
		0,
	);

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
			{
				"out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" },
				"dep_type": "dep_group",
			},
			{
				"out_point": { "tx_hash": dep_tx_hash, "index": "0x0" },
				"dep_type": "code",
			},
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", IDENTITY_CELL_CAPACITY),
				"lock": our_lock,
				"type": {
					"code_hash": type_code_hash,
					"hash_type": "data1",
					"args": type_id_args,
				},
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock,
				"type": null,
			}
		],
		"outputs_data": [
			format!("0x{}", hex::encode(&identity_data)),
			"0x",
		],
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;

	let signature = state.signer.sign(&tx_hash, inputs.len()).await?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}

pub async fn find_identity_cell_outpoint(
	state: &AppState,
	lock_args: &str,
) -> Result<Option<Value>, TxBuildError> {
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

	for cell in &cells.objects {
		if cell.output.lock.args.to_lowercase() == lock_args.to_lowercase() {
			return Ok(Some(json!(cell.out_point)));
		}
	}

	Ok(None)
}

/// The identity cell is locked under the child's lock_args, but the parent funds
/// and signs the transaction. Optionally creates a funding cell for the child.
///
/// When parent_lock_args is non-zero, the parent's identity cell is included as
/// a cell_dep so the type script can verify spending limit inheritance.
pub async fn build_spawn_sub_agent(
	state: &AppState,
	child_pubkey: &[u8; 33],
	child_lock_args: &str,
	spending_limit_shannons: u64,
	daily_limit_shannons: u64,
	parent_lock_args: &[u8; 20],
	revenue_share_bps: u16,
	initial_funding_shannons: u64,
) -> Result<(Value, String), TxBuildError> {
	let type_code_hash = std::env::var("AGENT_IDENTITY_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"AGENT_IDENTITY_TYPE_CODE_HASH not set; run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	let dep_tx_hash = std::env::var("AGENT_IDENTITY_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"AGENT_IDENTITY_DEP_TX_HASH not set; run scripts/deploy_contracts.sh first".into(),
		)
	})?;

	let parent_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	let child_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: child_lock_args.into(),
	};

	let cells = state.ckb.get_cells_by_lock(&parent_lock, 200).await?;
	let needed = IDENTITY_CELL_CAPACITY + initial_funding_shannons + ESTIMATED_FEE;

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
		inputs.push(json!({
			"previous_output": cell.out_point,
			"since": "0x0",
		}));
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

	let change_capacity = input_capacity - IDENTITY_CELL_CAPACITY - initial_funding_shannons - ESTIMATED_FEE;

	let identity_data = encode_identity_data(
		child_pubkey,
		spending_limit_shannons,
		daily_limit_shannons,
		parent_lock_args,
		revenue_share_bps,
		0,
		0,
	);

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

	let mut outputs = vec![json!({
		"capacity": format!("{:#x}", IDENTITY_CELL_CAPACITY),
		"lock": child_lock,
		"type": {
			"code_hash": type_code_hash,
			"hash_type": "data1",
			"args": type_id_args,
		},
	})];
	let mut outputs_data = vec![format!("0x{}", hex::encode(&identity_data))];

	if initial_funding_shannons > 0 {
		outputs.push(json!({
			"capacity": format!("{:#x}", initial_funding_shannons),
			"lock": child_lock,
			"type": null,
		}));
		outputs_data.push("0x".to_string());
	}

	outputs.push(json!({
		"capacity": format!("{:#x}", change_capacity),
		"lock": parent_lock,
		"type": null,
	}));
	outputs_data.push("0x".to_string());

	let mut cell_deps = vec![
		json!({ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" }),
		json!({ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" }),
	];

	if *parent_lock_args != [0u8; 20] {
		let parent_args_hex = format!("0x{}", hex::encode(parent_lock_args));
		if let Some(parent_outpoint) = find_identity_cell_outpoint(state, &parent_args_hex).await? {
			cell_deps.push(json!({ "out_point": parent_outpoint, "dep_type": "code" }));
		} else {
			return Err(TxBuildError::CellNotFound(format!(
				"parent identity cell not found for lock_args {}",
				hex::encode(parent_lock_args)
			)));
		}
	}

	let mut tx = json!({
		"version": "0x0",
		"cell_deps": cell_deps,
		"header_deps": [],
		"inputs": inputs,
		"outputs": outputs,
		"outputs_data": outputs_data,
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;

	let signature = state.signer.sign(&tx_hash, inputs.len()).await?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}

pub async fn build_deploy_binary(
	state: &AppState,
	binary: Vec<u8>,
) -> Result<(Value, String, String), TxBuildError> {
	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	// CKB rule: cell.capacity (shannons) >= occupied_bytes * 10^8.
	let occupied_bytes = 8 + 53 + binary.len() as u64;
	let required_capacity = occupied_bytes * 100_000_000;
	let needed = required_capacity + 1_000_000;

	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	for cell in &cells.objects {
		if cell.output.type_script.is_some() {
			continue;
		}
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

	let change_capacity = input_capacity - required_capacity - 1_000_000;
	let code_hash = {
		let hash = blake2b_256(&binary);
		format!("0x{}", hex::encode(hash))
	};

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
		"cell_deps": [{
			"out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" },
			"dep_type": "dep_group",
		}],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", required_capacity),
				"lock": our_lock.clone(),
				"type": null,
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock,
				"type": null,
			}
		],
		"outputs_data": [
			format!("0x{}", hex::encode(&binary)),
			"0x",
		],
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;

	let signature = state.signer.sign(&tx_hash, inputs.len()).await?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash, code_hash))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encode_identity_data_layout() {
		let pubkey = [0xEE; 33];
		let parent = [0xFF; 20];
		let data = encode_identity_data(&pubkey, 300_000_000, 1_000_000_000, &parent, 2000, 0, 0);
		assert_eq!(data.len(), IDENTITY_DATA_SIZE);
		assert_eq!(data[0], 0, "version must be 0");
		assert_eq!(&data[1..34], &pubkey);
		let spending = u64::from_le_bytes(data[34..42].try_into().unwrap());
		assert_eq!(spending, 300_000_000);
		let daily = u64::from_le_bytes(data[42..50].try_into().unwrap());
		assert_eq!(daily, 1_000_000_000);
		assert_eq!(&data[50..70], &parent);
		let bps = u16::from_le_bytes(data[70..72].try_into().unwrap());
		assert_eq!(bps, 2000);
		let daily_spent = u64::from_le_bytes(data[72..80].try_into().unwrap());
		assert_eq!(daily_spent, 0);
		let last_epoch = u64::from_le_bytes(data[80..88].try_into().unwrap());
		assert_eq!(last_epoch, 0);
	}

	#[test]
	fn decode_identity_roundtrip() {
		let pubkey = [0xEE; 33];
		let parent = [0xFF; 20];
		let data = encode_identity_data(&pubkey, 300_000_000, 1_000_000_000, &parent, 2000, 50_000_000, 42);
		let decoded = decode_identity_data(&data).unwrap();
		assert_eq!(decoded.pubkey, pubkey);
		assert_eq!(decoded.spending_limit_shannons, 300_000_000);
		assert_eq!(decoded.daily_limit_shannons, 1_000_000_000);
		assert_eq!(decoded.parent_lock_args, parent);
		assert_eq!(decoded.revenue_share_bps, 2000);
		assert_eq!(decoded.daily_spent, 50_000_000);
		assert_eq!(decoded.last_reset_epoch, 42);
	}

	#[test]
	fn fresh_accumulator() {
		let pubkey = [0xAA; 33];
		let parent = [0u8; 20];
		let data = encode_identity_data(&pubkey, 100_000_000, 500_000_000, &parent, 0, 0, 0);
		let decoded = decode_identity_data(&data).unwrap();
		assert_eq!(decoded.daily_spent, 0);
		assert_eq!(decoded.last_reset_epoch, 0);
	}

	#[test]
	fn root_agent_has_zero_parent() {
		let pubkey = [0xDD; 33];
		let zero_parent = [0u8; 20];
		let data = encode_identity_data(&pubkey, 100_000_000, 500_000_000, &zero_parent, 0, 0, 0);
		let decoded = decode_identity_data(&data).unwrap();
		assert_eq!(decoded.parent_lock_args, [0u8; 20]);
		assert_eq!(decoded.revenue_share_bps, 0);
	}

	#[test]
	fn decode_identity_rejects_short_data() {
		let data = vec![0u8; 10];
		assert!(decode_identity_data(&data).is_err());
	}

	#[test]
	fn decode_identity_rejects_unknown_version() {
		let mut data = vec![0u8; 88];
		data[0] = 99;
		assert!(decode_identity_data(&data).is_err());
	}

	#[test]
	fn blake2b_256_ckb_personalization() {
		let hash = blake2b_256(&[]);
		assert_eq!(hash.len(), 32);
		assert!(!hash.iter().all(|&b| b == 0));
	}

	#[test]
	fn blake2b_256_deterministic() {
		let data = b"test data";
		let h1 = blake2b_256(data);
		let h2 = blake2b_256(data);
		assert_eq!(h1, h2, "same input must produce same hash");
	}

	#[test]
	fn calculate_type_id_basic() {
		let tx_hash = "0x".to_owned() + &"ab".repeat(32);
		let type_id = calculate_type_id(&tx_hash, 0, 0, 0).unwrap();
		assert!(type_id.starts_with("0x"));
		assert_eq!(type_id.len(), 66, "type_id should be 0x + 64 hex chars");
	}

	#[test]
	fn calculate_type_id_deterministic() {
		let tx_hash = "0x".to_owned() + &"ff".repeat(32);
		let id1 = calculate_type_id(&tx_hash, 0, 0, 0).unwrap();
		let id2 = calculate_type_id(&tx_hash, 0, 0, 0).unwrap();
		assert_eq!(id1, id2);
	}

	#[test]
	fn calculate_type_id_changes_with_index() {
		let tx_hash = "0x".to_owned() + &"ab".repeat(32);
		let id_0 = calculate_type_id(&tx_hash, 0, 0, 0).unwrap();
		let id_1 = calculate_type_id(&tx_hash, 1, 0, 0).unwrap();
		assert_ne!(id_0, id_1, "different input index must produce different type_id");
	}

	#[test]
	fn calculate_type_id_changes_with_output_index() {
		let tx_hash = "0x".to_owned() + &"ab".repeat(32);
		let id_0 = calculate_type_id(&tx_hash, 0, 0, 0).unwrap();
		let id_1 = calculate_type_id(&tx_hash, 0, 0, 1).unwrap();
		assert_ne!(id_0, id_1, "different output index must produce different type_id");
	}

	#[test]
	fn calculate_type_id_rejects_bad_hash() {
		let result = calculate_type_id("0xdeadbeef", 0, 0, 0);
		assert!(result.is_err(), "short tx_hash should be rejected");
	}
}
