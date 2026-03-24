use serde_json::{json, Value};

use crate::{
	errors::TxBuildError,
	state::{ckb_to_shannons, parse_capacity_hex, AppState, SECP256K1_DEP_TX_HASH},
};

use super::{
	gather_fee_inputs, molecule::compute_raw_tx_hash, our_lock, placeholder_witnesses,
	signing::inject_witness, MIN_CELL_CAPACITY,
};

const ESTIMATED_FEE: u64 = 2_000_000;
const POOL_CELL_CAPACITY: u64 = 127 * 100_000_000;

fn amm_type_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("MOCK_AMM_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"MOCK_AMM_TYPE_CODE_HASH not set; run scripts/deploy_contracts.sh mock_amm first"
				.into(),
		)
	})?;
	let dep_tx_hash = std::env::var("MOCK_AMM_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"MOCK_AMM_DEP_TX_HASH not set; run scripts/deploy_contracts.sh mock_amm first".into(),
		)
	})?;
	Ok((code_hash, dep_tx_hash))
}

fn encode_pool_data(reserve_ckb: u128, reserve_token: u128) -> Vec<u8> {
	let mut data = Vec::with_capacity(33);
	data.push(0u8);
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

fn calculate_swap_output(reserve_in: u128, reserve_out: u128, amount_in: u128) -> u128 {
	let numerator = reserve_out.saturating_mul(amount_in);
	let denominator = reserve_in.saturating_add(amount_in);
	if denominator == 0 {
		return 0;
	}
	numerator / denominator
}

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
	let data_hex = cell.data.map(|d| d.content).unwrap_or_else(|| "0x".into());
	let pool_data = hex::decode(data_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad pool data: {e}")))?;

	let (old_reserve_ckb, old_reserve_token) = parse_pool_data(&pool_data)?;
	let amount_in = amount_ckb_shannons as u128;
	let tokens_out = calculate_swap_output(old_reserve_ckb, old_reserve_token, amount_in);
	if tokens_out == 0 {
		return Err(TxBuildError::Rpc("swap would produce zero tokens".into()));
	}

	let min_tokens = tokens_out * (10_000 - slippage_bps as u128) / 10_000;
	if tokens_out < min_tokens {
		return Err(TxBuildError::Rpc("slippage tolerance exceeded".into()));
	}

	let new_reserve_ckb = old_reserve_ckb + amount_in;
	let new_reserve_token = old_reserve_token
		.checked_sub(tokens_out)
		.ok_or_else(|| TxBuildError::Rpc("pool token reserve underflow".into()))?;
	let new_pool_data = encode_pool_data(new_reserve_ckb, new_reserve_token);
	let new_pool_capacity = pool_capacity + amount_ckb_shannons;

	let needed = amount_ckb_shannons + ESTIMATED_FEE;
	let (fee_inputs, fee_capacity) = gather_fee_inputs(state, needed).await?;
	let change_capacity = fee_capacity - amount_ckb_shannons - ESTIMATED_FEE;

	let mut all_inputs = vec![json!({
		"previous_output": { "tx_hash": pool_tx_hash, "index": format!("{:#x}", pool_index) },
		"since": "0x0",
	})];
	all_inputs.extend(fee_inputs);

	let witnesses = placeholder_witnesses(all_inputs.len());

	let mut tx = json!({
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

	let tx_hash = compute_raw_tx_hash(&tx)?;
	let signature = state.signer.sign(&tx_hash, all_inputs.len()).await?;
	inject_witness(&mut tx, &signature);
	Ok((tx, tx_hash))
}

pub async fn build_create_pool(
	state: &AppState,
	seed_ckb_shannons: u64,
	seed_token_amount: u128,
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = amm_type_env()?;

	if seed_ckb_shannons < MIN_CELL_CAPACITY {
		return Err(TxBuildError::InsufficientCapacity {
			need: MIN_CELL_CAPACITY,
			have: seed_ckb_shannons,
		});
	}

	let pool_capacity = POOL_CELL_CAPACITY + seed_ckb_shannons;
	let pool_data = encode_pool_data(seed_ckb_shannons as u128, seed_token_amount);
	let (inputs, input_capacity) = gather_fee_inputs(state, pool_capacity + ESTIMATED_FEE).await?;
	let change_capacity = input_capacity - pool_capacity - ESTIMATED_FEE;
	let witnesses = placeholder_witnesses(inputs.len());

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

	let tx_hash = compute_raw_tx_hash(&tx)?;
	let signature = state.signer.sign(&tx_hash, inputs.len()).await?;
	inject_witness(&mut tx, &signature);
	Ok((tx, tx_hash))
}
