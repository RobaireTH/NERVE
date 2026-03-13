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
// Minimum capacity for a pool cell: cap(8) + lock(53) + type(33) + data(33) = 127 CKB.
const POOL_CELL_CAPACITY: u64 = 127 * 100_000_000;
// Minimum capacity for a plain secp256k1 cell.
const MIN_CELL_CAPACITY: u64 = 61 * 100_000_000;

fn amm_type_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("MOCK_AMM_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"MOCK_AMM_TYPE_CODE_HASH not set — run scripts/deploy_contracts.sh mock_amm first"
				.into(),
		)
	})?;
	let dep_tx_hash = std::env::var("MOCK_AMM_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"MOCK_AMM_DEP_TX_HASH not set — run scripts/deploy_contracts.sh mock_amm first".into(),
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

/// Encode pool cell data: version(1) + reserve_ckb(16) + reserve_token(16) = 33 bytes.
fn encode_pool_data(reserve_ckb: u128, reserve_token: u128) -> Vec<u8> {
	let mut data = Vec::with_capacity(33);
	data.push(0u8); // version
	data.extend_from_slice(&reserve_ckb.to_le_bytes());
	data.extend_from_slice(&reserve_token.to_le_bytes());
	data
}

fn parse_pool_data(data: &[u8]) -> Result<(u128, u128), TxBuildError> {
	if data.len() < 33 {
		return Err(TxBuildError::Rpc("pool cell data too short".into()));
	}
	let reserve_ckb = u128::from_le_bytes(data[1..17].try_into().unwrap());
	let reserve_token = u128::from_le_bytes(data[17..33].try_into().unwrap());
	Ok((reserve_ckb, reserve_token))
}

/// Calculates the output amount for a constant-product swap: dy = y * dx / (x + dx).
fn calculate_swap_output(reserve_in: u128, reserve_out: u128, amount_in: u128) -> u128 {
	// dy = (y * dx) / (x + dx)
	let numerator = reserve_out.saturating_mul(amount_in);
	let denominator = reserve_in.saturating_add(amount_in);
	if denominator == 0 {
		return 0;
	}
	numerator / denominator
}

/// Builds a swap transaction against the mock AMM pool.
///
/// The swap consumes the pool cell (input) and recreates it with updated reserves (output).
/// For a CKB→TOKEN swap the agent sends CKB into the pool and the pool "sends" tokens
/// by updating the reserve balance. The token amount is tracked in the pool data only
/// (no UDT cell needed for the demo — the reserve delta is the proof of swap).
pub async fn build_swap(
	state: &AppState,
	pool_tx_hash: &str,
	pool_index: u32,
	amount_ckb_shannons: u64,
	slippage_bps: u32,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = amm_type_env()?;

	if amount_ckb_shannons > state.spending_limit_shannons {
		return Err(TxBuildError::SpendingLimitExceeded {
			requested: amount_ckb_shannons,
			limit: state.spending_limit_shannons,
		});
	}

	// Fetch the current pool cell.
	let result = state.ckb.get_live_cell(pool_tx_hash, pool_index).await?;
	if result.status != "live" {
		return Err(TxBuildError::CellNotFound(format!(
			"pool {}:{} status={}",
			pool_tx_hash, pool_index, result.status
		)));
	}
	let cell = result
		.cell
		.ok_or_else(|| TxBuildError::CellNotFound(format!("{pool_tx_hash}:{pool_index}")))?;
	let pool_capacity = parse_capacity_hex(&cell.output.capacity)?;
	let data_hex = cell
		.data
		.map(|d| d.content)
		.unwrap_or_else(|| "0x".into());
	let pool_data = hex::decode(data_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad pool data: {e}")))?;

	let (old_reserve_ckb, old_reserve_token) = parse_pool_data(&pool_data)?;

	// Calculate swap output.
	let amount_in = amount_ckb_shannons as u128;
	let tokens_out = calculate_swap_output(old_reserve_ckb, old_reserve_token, amount_in);

	if tokens_out == 0 {
		return Err(TxBuildError::Rpc("swap would produce zero tokens".into()));
	}

	// Apply slippage tolerance: reduce output to the minimum acceptable amount.
	// This protects against front-running by accepting a worse rate up front.
	let min_tokens = tokens_out * (10_000 - slippage_bps as u128) / 10_000;
	if min_tokens == 0 {
		return Err(TxBuildError::Rpc("slippage-adjusted output is zero".into()));
	}

	// Use min_tokens as the actual swap output — the pool keeps the surplus.
	let actual_tokens_out = min_tokens;

	// New reserves after swap.
	let new_reserve_ckb = old_reserve_ckb + amount_in;
	let new_reserve_token = old_reserve_token - actual_tokens_out;
	let new_pool_data = encode_pool_data(new_reserve_ckb, new_reserve_token);

	// The pool cell capacity increases by the CKB sent in.
	let new_pool_capacity = pool_capacity + amount_ckb_shannons;

	// Gather agent cells for the CKB being swapped + fee.
	let needed = amount_ckb_shannons + ESTIMATED_FEE + MIN_CELL_CAPACITY;
	let agent_lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&agent_lock, 200).await?;

	let mut fee_inputs = Vec::new();
	let mut fee_capacity: u64 = 0;
	for cell in &cells.objects {
		// Skip typed cells to avoid consuming protocol cells (job, reputation, etc.).
		if cell.output.type_script.is_some() {
			continue;
		}
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		fee_inputs.push(json!({ "previous_output": cell.out_point, "since": "0x0" }));
		fee_capacity += cap;
		if fee_capacity >= needed {
			break;
		}
	}
	if fee_capacity < needed {
		return Err(TxBuildError::InsufficientFunds {
			need: needed as f64 / 1e8,
			have: fee_capacity as f64 / 1e8,
		});
	}

	let change_capacity = fee_capacity - amount_ckb_shannons - ESTIMATED_FEE;

	// Build inputs: pool cell first, then agent fee cells.
	let mut all_inputs = vec![json!({
		"previous_output": { "tx_hash": pool_tx_hash, "index": format!("{:#x}", pool_index) },
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
				"capacity": format!("{:#x}", new_pool_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(&new_pool_data)), "0x"],
		"witnesses": witnesses,
	});

	// Sign.
	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();
	let signature = sign_tx(&tx_hash, &state.private_key, all_inputs.len())?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}

/// Builds a transaction to create the initial AMM pool cell with seed liquidity.
pub async fn build_create_pool(
	state: &AppState,
	seed_ckb_shannons: u64,
	seed_token_amount: u128,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = amm_type_env()?;

	let pool_capacity = POOL_CELL_CAPACITY + seed_ckb_shannons;
	let pool_data = encode_pool_data(seed_ckb_shannons as u128, seed_token_amount);

	// Gather enough cells for pool capacity + fee + change.
	let needed = pool_capacity + ESTIMATED_FEE + MIN_CELL_CAPACITY;
	let agent_lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&agent_lock, 200).await?;

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

	let change_capacity = input_capacity - pool_capacity - ESTIMATED_FEE;
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
				"capacity": format!("{:#x}", pool_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(&pool_data)), "0x"],
		"witnesses": witnesses,
	});

	let accepted = state.ckb.test_tx_pool_accept(&tx).await?;
	let tx_hash = accepted["tx_hash"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("test_tx_pool_accept: missing tx_hash".into()))?
		.to_owned();
	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}
