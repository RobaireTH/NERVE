use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{parse_capacity_hex, AppState, SECP256K1_CODE_HASH, SECP256K1_DEP_TX_HASH, SECP256K1_HASH_TYPE},
};

use super::molecule::compute_raw_tx_hash;
use super::signing::{inject_witness, placeholder_witness, sign_tx};

use super::MIN_CELL_CAPACITY;
// Estimated fee for a simple 1-input, 2-output transfer (in shannons).
const ESTIMATED_FEE: u64 = 1_000_000;

pub async fn build_transfer(
	state: &AppState,
	to_lock_args: &str,
	amount_shannons: u64,
) -> Result<(Value, String), TxBuildError> {
	let args_bytes = hex::decode(to_lock_args.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::InvalidLockArgs(format!("bad hex: {e}")))?;
	if args_bytes.len() != 20 {
		return Err(TxBuildError::InvalidLockArgs(format!(
			"lock_args must be 20 bytes, got {}",
			args_bytes.len()
		)));
	}

	if amount_shannons < MIN_CELL_CAPACITY {
		return Err(TxBuildError::InsufficientCapacity {
			need: MIN_CELL_CAPACITY,
			have: amount_shannons,
		});
	}
	if amount_shannons > state.spending_limit_shannons {
		return Err(TxBuildError::SpendingLimitExceeded {
			requested: amount_shannons,
			limit: state.spending_limit_shannons,
		});
	}

	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;
	let needed = amount_shannons + ESTIMATED_FEE + MIN_CELL_CAPACITY; // output + fee + change cell

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	for cell in &cells.objects {
		if cell.output.type_script.is_some() {
			continue;
		}
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

	let change_capacity = input_capacity - amount_shannons - ESTIMATED_FEE;

	let recipient_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: to_lock_args.into(),
	};

	let outputs = json!([
		{
			"capacity": format!("{:#x}", amount_shannons),
			"lock": recipient_lock,
			"type": null,
		},
		{
			"capacity": format!("{:#x}", change_capacity),
			"lock": our_lock,
			"type": null,
		}
	]);

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
			"out_point": {
				"tx_hash": SECP256K1_DEP_TX_HASH,
				"index": "0x0",
			},
			"dep_type": "dep_group",
		}],
		"header_deps": [],
		"inputs": inputs,
		"outputs": outputs,
		"outputs_data": ["0x", "0x"],
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;

	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}
