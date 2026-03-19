pub mod badge;
pub mod capability;
pub mod identity;
pub mod intents;
pub mod job;
pub mod reputation;
pub(crate) mod molecule;
pub(crate) mod signing;
mod transfer;

use serde_json::Value;

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{parse_capacity_hex, AppState, SECP256K1_CODE_HASH, SECP256K1_HASH_TYPE},
};
use signing::placeholder_witness;

pub(crate) const MIN_CELL_CAPACITY: u64 = 61 * 100_000_000;

pub(crate) fn our_lock(state: &AppState) -> Script {
	Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	}
}

pub(crate) fn placeholder_witnesses(count: usize) -> Vec<Value> {
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

pub(crate) async fn gather_fee_inputs(
	state: &AppState,
	needed: u64,
) -> Result<(Vec<Value>, u64), TxBuildError> {
	let lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&lock, 200).await?;

	let mut inputs = Vec::new();
	let mut capacity: u64 = 0;
	for cell in &cells.objects {
		if cell.output.type_script.is_some() {
			continue;
		}
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		inputs.push(serde_json::json!({ "previous_output": cell.out_point, "since": "0x0" }));
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
