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

use super::signing::{inject_witness, placeholder_witness, sign_tx};

// Agent identity cell data layout (50 bytes):
//   [0]      version = 0
//   [1..34]  compressed secp256k1 pubkey (33 bytes)
//   [34..42] spending_limit_per_tx as u64 LE
//   [42..50] daily_limit as u64 LE
const IDENTITY_DATA_SIZE: usize = 50;

// Minimum capacity shannons for an identity cell:
//   capacity(8) + lock(53) + type_script(33) + data(50) = 144 bytes
// Add a 56-byte buffer to round up to 200 CKB for safety.
const IDENTITY_CELL_CAPACITY: u64 = 200 * 100_000_000;

// Estimated fee for the spawn transaction (shannons).
const ESTIMATED_FEE: u64 = 1_000_000;

/// Serializes the agent identity cell data in the simple 50-byte layout.
pub fn encode_identity_data(
	pubkey: &[u8; 33],
	spending_limit_shannons: u64,
	daily_limit_shannons: u64,
) -> Vec<u8> {
	let mut data = Vec::with_capacity(IDENTITY_DATA_SIZE);
	data.push(0u8); // version
	data.extend_from_slice(pubkey);
	data.extend_from_slice(&spending_limit_shannons.to_le_bytes());
	data.extend_from_slice(&daily_limit_shannons.to_le_bytes());
	data
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
	for cell in &cells.objects {
		let cap = parse_capacity_hex(&cell.output.capacity)?;
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
			// secp256k1-blake2b lock dep.
			{
				"out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" },
				"dep_type": "dep_group",
			},
			// Agent identity type script dep.
			{
				"out_point": { "tx_hash": dep_tx_hash, "index": "0x0" },
				"dep_type": "code",
			},
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			// Identity cell output.
			{
				"capacity": format!("{:#x}", IDENTITY_CELL_CAPACITY),
				"lock": our_lock,
				"type": {
					"code_hash": type_code_hash,
					"hash_type": "data1",
					"args": "0x",
				},
			},
			// Change output.
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

	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();

	let signature = sign_tx(&tx_hash, &state.private_key)?;
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

	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();

	let signature = sign_tx(&tx_hash, &state.private_key)?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash, code_hash))
}
