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
use super::signing::{inject_witness, placeholder_witness, sign_tx};

// Agent identity cell data layout:
//
// v0 (50 bytes — original):
//   [0]      version = 0
//   [1..34]  compressed secp256k1 pubkey (33 bytes)
//   [34..42] spending_limit_per_tx as u64 LE
//   [42..50] daily_limit as u64 LE
//
// v1 (72 bytes — sub-agent delegation):
//   [0]      version = 1
//   [1..34]  compressed secp256k1 pubkey (33 bytes)
//   [34..42] spending_limit_per_tx as u64 LE
//   [42..50] daily_limit as u64 LE
//   [50..70] parent_lock_args (20 bytes, all zeros = root agent)
//   [70..72] revenue_share_bps (u16 LE, basis points: 1000 = 10%)
const IDENTITY_V0_DATA_SIZE: usize = 50;
const IDENTITY_V1_DATA_SIZE: usize = 72;

// Minimum capacity shannons for an identity cell:
//   capacity(8) + lock(53) + type_script(33 + 32 args) + data(50) = 176 bytes
// Add buffer to round up to 232 CKB for safety.
const IDENTITY_CELL_CAPACITY: u64 = 232 * 100_000_000;

// Estimated fee for the spawn transaction (shannons).
const ESTIMATED_FEE: u64 = 1_000_000;

/// Parsed agent identity cell data, version-aware.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentityData {
	pub version: u8,
	pub pubkey: [u8; 33],
	pub spending_limit_shannons: u64,
	pub daily_limit_shannons: u64,
	/// Parent agent's lock_args (v1 only). None for v0, Some([0;20]) for root v1 agents.
	pub parent_lock_args: Option<[u8; 20]>,
	/// Revenue share in basis points (v1 only). None for v0.
	pub revenue_share_bps: Option<u16>,
}

/// Serializes a v0 agent identity cell data (50 bytes).
pub fn encode_identity_data(
	pubkey: &[u8; 33],
	spending_limit_shannons: u64,
	daily_limit_shannons: u64,
) -> Vec<u8> {
	let mut data = Vec::with_capacity(IDENTITY_V0_DATA_SIZE);
	data.push(0u8); // version
	data.extend_from_slice(pubkey);
	data.extend_from_slice(&spending_limit_shannons.to_le_bytes());
	data.extend_from_slice(&daily_limit_shannons.to_le_bytes());
	data
}

/// Serializes a v1 agent identity cell data (72 bytes) with parent delegation fields.
pub fn encode_identity_data_v1(
	pubkey: &[u8; 33],
	spending_limit_shannons: u64,
	daily_limit_shannons: u64,
	parent_lock_args: &[u8; 20],
	revenue_share_bps: u16,
) -> Vec<u8> {
	let mut data = Vec::with_capacity(IDENTITY_V1_DATA_SIZE);
	data.push(1u8); // version
	data.extend_from_slice(pubkey);
	data.extend_from_slice(&spending_limit_shannons.to_le_bytes());
	data.extend_from_slice(&daily_limit_shannons.to_le_bytes());
	data.extend_from_slice(parent_lock_args);
	data.extend_from_slice(&revenue_share_bps.to_le_bytes());
	data
}

/// Decodes identity cell data, handling both v0 (50 bytes) and v1 (72 bytes) layouts.
pub fn decode_identity_data(data: &[u8]) -> Result<IdentityData, TxBuildError> {
	if data.len() < IDENTITY_V0_DATA_SIZE {
		return Err(TxBuildError::Rpc(format!(
			"identity data too short: {} bytes, need at least {}",
			data.len(),
			IDENTITY_V0_DATA_SIZE
		)));
	}

	let version = data[0];
	let mut pubkey = [0u8; 33];
	pubkey.copy_from_slice(&data[1..34]);
	let spending_limit_shannons = u64::from_le_bytes(data[34..42].try_into().unwrap());
	let daily_limit_shannons = u64::from_le_bytes(data[42..50].try_into().unwrap());

	match version {
		0 => Ok(IdentityData {
			version,
			pubkey,
			spending_limit_shannons,
			daily_limit_shannons,
			parent_lock_args: None,
			revenue_share_bps: None,
		}),
		1 => {
			if data.len() < IDENTITY_V1_DATA_SIZE {
				return Err(TxBuildError::Rpc(format!(
					"v1 identity data too short: {} bytes, need {}",
					data.len(),
					IDENTITY_V1_DATA_SIZE
				)));
			}
			let mut parent_lock_args = [0u8; 20];
			parent_lock_args.copy_from_slice(&data[50..70]);
			let revenue_share_bps = u16::from_le_bytes(data[70..72].try_into().unwrap());
			Ok(IdentityData {
				version,
				pubkey,
				spending_limit_shannons,
				daily_limit_shannons,
				parent_lock_args: Some(parent_lock_args),
				revenue_share_bps: Some(revenue_share_bps),
			})
		}
		_ => Err(TxBuildError::Rpc(format!("unknown identity version: {version}"))),
	}
}

/// Computes blake2b-256 of data with CKB personalization — used for code_hash derivation.
pub fn blake2b_256(data: &[u8]) -> [u8; 32] {
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(data);
	let mut out = [0u8; 32];
	hasher.finalize(&mut out);
	out
}

/// Calculates the CKB Type ID from the first input's CellInput molecule encoding
/// and the output index of the type-id-bearing cell.
///
/// type_id = blake2b(since(8) || prev_tx_hash(32) || prev_index(4) || output_index(8))
///
/// The `since`, `prev_tx_hash`, and `prev_index` come from inputs[0].
/// The `output_index` is where the type-id cell appears in the transaction outputs.
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

/// Builds, signs, and returns a spawn-agent transaction.
///
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
			"AGENT_IDENTITY_TYPE_CODE_HASH not set — run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	let dep_tx_hash = std::env::var("AGENT_IDENTITY_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"AGENT_IDENTITY_DEP_TX_HASH not set — run scripts/deploy_contracts.sh first".into(),
		)
	})?;

	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	// Gather input cells.
	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;
	let needed = IDENTITY_CELL_CAPACITY + ESTIMATED_FEE;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	// Track the first input's outpoint for Type ID calculation.
	let mut first_input_tx_hash: Option<String> = None;
	let mut first_input_index: u32 = 0;
	for cell in &cells.objects {
		// Skip typed cells to avoid consuming protocol cells (job, reputation, etc.).
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

	// Calculate Type ID: blake2b(cell_input_molecule || output_index).
	// The identity cell is at output index 0.
	let type_id_args = calculate_type_id(&first_tx_hash, first_input_index, 0, 0)?;

	let change_capacity = input_capacity - IDENTITY_CELL_CAPACITY - ESTIMATED_FEE;

	let identity_data = encode_identity_data(
		compressed_pubkey,
		spending_limit_shannons,
		daily_limit_shannons,
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

	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}

/// Builds, signs, and returns a spawn-sub-agent transaction.
///
/// Creates a v1 identity cell for a child agent with parent delegation fields.
/// The identity cell is locked under the child's lock_args, but the parent funds
/// and signs the transaction. Optionally creates a funding cell for the child.
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
			"AGENT_IDENTITY_TYPE_CODE_HASH not set — run scripts/deploy_contracts.sh first".into(),
		)
	})?;
	let dep_tx_hash = std::env::var("AGENT_IDENTITY_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"AGENT_IDENTITY_DEP_TX_HASH not set — run scripts/deploy_contracts.sh first".into(),
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

	// Gather input cells from parent.
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

	let identity_data = encode_identity_data_v1(
		child_pubkey,
		spending_limit_shannons,
		daily_limit_shannons,
		parent_lock_args,
		revenue_share_bps,
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

	// Build outputs: identity cell (child lock) + optional funding cell (child lock) + change (parent lock).
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
		"outputs": outputs,
		"outputs_data": outputs_data,
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;

	// Parent signs the transaction.
	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}

/// Builds, signs, and returns a data-cell deployment transaction for a contract binary.
pub async fn build_deploy_binary(
	state: &AppState,
	binary: Vec<u8>,
) -> Result<(Value, String, String), TxBuildError> {
	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	// Capacity: capacity(8) + lock(53) + data(binary.len()) bytes → * 10^8 shannons/CKB = bytes CKB.
	// But CKB's capacity field stores shannons. 1 byte = 1 CKB = 10^8 shannons? No.
	// Correct: minimum_capacity_shannons = occupied_bytes * 10^8 ... No that's still wrong.
	// The correct formula: 1 shannon per byte. So 144 bytes needs 144 shannons minimum? That's tiny.
	//
	// Actually: CKB stores capacity in shannons, and the rule is:
	//   cell.capacity >= total_occupied_bytes (in units where 1 CKB = 10^8 shannons).
	// Meaning: cell.capacity_in_ckb >= total_occupied_bytes.
	// Meaning: cell.capacity_in_shannons >= total_occupied_bytes * 10^8.
	// So 100 bytes needs 100 CKB = 100 * 10^8 shannons.
	let occupied_bytes = 8 + 53 + binary.len() as u64; // capacity + lock_script + data
	let required_capacity = occupied_bytes * 100_000_000; // shannons (1 byte = 1 CKB = 10^8 shannons)
	let needed = required_capacity + 1_000_000; // + fee

	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	for cell in &cells.objects {
		// Skip typed cells to avoid consuming protocol cells (job, reputation, etc.).
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

	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash, code_hash))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encode_identity_data_v0_layout() {
		let pubkey = [0xAA; 33];
		let data = encode_identity_data(&pubkey, 100_000_000, 500_000_000);
		assert_eq!(data.len(), IDENTITY_V0_DATA_SIZE);
		assert_eq!(data[0], 0, "version must be 0");
		assert_eq!(&data[1..34], &pubkey);
		let spending = u64::from_le_bytes(data[34..42].try_into().unwrap());
		assert_eq!(spending, 100_000_000);
		let daily = u64::from_le_bytes(data[42..50].try_into().unwrap());
		assert_eq!(daily, 500_000_000);
	}

	#[test]
	fn encode_identity_data_v1_layout() {
		let pubkey = [0xBB; 33];
		let parent = [0xCC; 20];
		let data = encode_identity_data_v1(&pubkey, 200_000_000, 800_000_000, &parent, 1000);
		assert_eq!(data.len(), IDENTITY_V1_DATA_SIZE);
		assert_eq!(data[0], 1, "version must be 1");
		assert_eq!(&data[1..34], &pubkey);
		let spending = u64::from_le_bytes(data[34..42].try_into().unwrap());
		assert_eq!(spending, 200_000_000);
		let daily = u64::from_le_bytes(data[42..50].try_into().unwrap());
		assert_eq!(daily, 800_000_000);
		assert_eq!(&data[50..70], &parent);
		let bps = u16::from_le_bytes(data[70..72].try_into().unwrap());
		assert_eq!(bps, 1000);
	}

	#[test]
	fn decode_identity_v0_roundtrip() {
		let pubkey = [0xAA; 33];
		let data = encode_identity_data(&pubkey, 100_000_000, 500_000_000);
		let decoded = decode_identity_data(&data).unwrap();
		assert_eq!(decoded.version, 0);
		assert_eq!(decoded.pubkey, pubkey);
		assert_eq!(decoded.spending_limit_shannons, 100_000_000);
		assert_eq!(decoded.daily_limit_shannons, 500_000_000);
		assert!(decoded.parent_lock_args.is_none());
		assert!(decoded.revenue_share_bps.is_none());
	}

	#[test]
	fn decode_identity_v1_roundtrip() {
		let pubkey = [0xBB; 33];
		let parent = [0xCC; 20];
		let data = encode_identity_data_v1(&pubkey, 200_000_000, 800_000_000, &parent, 1500);
		let decoded = decode_identity_data(&data).unwrap();
		assert_eq!(decoded.version, 1);
		assert_eq!(decoded.pubkey, pubkey);
		assert_eq!(decoded.spending_limit_shannons, 200_000_000);
		assert_eq!(decoded.daily_limit_shannons, 800_000_000);
		assert_eq!(decoded.parent_lock_args, Some(parent));
		assert_eq!(decoded.revenue_share_bps, Some(1500));
	}

	#[test]
	fn decode_identity_v0_backward_compat() {
		// A v0 identity cell should decode without parent/share fields.
		let pubkey = [0x11; 33];
		let data = encode_identity_data(&pubkey, 50_000_000, 100_000_000);
		let decoded = decode_identity_data(&data).unwrap();
		assert_eq!(decoded.version, 0);
		assert!(decoded.parent_lock_args.is_none());
		assert!(decoded.revenue_share_bps.is_none());
	}

	#[test]
	fn decode_identity_rejects_short_data() {
		let data = vec![0u8; 10];
		assert!(decode_identity_data(&data).is_err());
	}

	#[test]
	fn decode_identity_rejects_unknown_version() {
		let mut data = vec![0u8; 72];
		data[0] = 99; // unknown version
		assert!(decode_identity_data(&data).is_err());
	}

	#[test]
	fn decode_identity_v1_rejects_short_data() {
		// 50 bytes with version=1 should fail (needs 72).
		let mut data = vec![0u8; 50];
		data[0] = 1;
		assert!(decode_identity_data(&data).is_err());
	}

	#[test]
	fn v1_root_agent_has_zero_parent() {
		let pubkey = [0xDD; 33];
		let zero_parent = [0u8; 20];
		let data = encode_identity_data_v1(&pubkey, 100_000_000, 500_000_000, &zero_parent, 0);
		let decoded = decode_identity_data(&data).unwrap();
		assert_eq!(decoded.parent_lock_args, Some([0u8; 20]));
		assert_eq!(decoded.revenue_share_bps, Some(0));
	}

	#[test]
	fn blake2b_256_ckb_personalization() {
		// Known CKB hash: blake2b("ckb-default-hash", []) should produce the standard empty hash.
		let hash = blake2b_256(&[]);
		// The hash should be deterministic and 32 bytes.
		assert_eq!(hash.len(), 32);
		// Verify it's not all zeros (the hash of empty with CKB personalization is non-trivial).
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
