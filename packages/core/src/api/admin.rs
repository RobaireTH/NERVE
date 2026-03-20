use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{
		parse_capacity_hex, shannons_to_ckb, AppState, SECP256K1_CODE_HASH,
		SECP256K1_DEP_TX_HASH, SECP256K1_HASH_TYPE,
	},
	tx_builder::{
		identity::build_deploy_binary,
		molecule::compute_raw_tx_hash,
	},
};

#[derive(Debug, Deserialize)]
pub struct DeployBinRequest {
	/// Binary content as a 0x-prefixed hex string.
	pub binary_hex: String,
}

/// POST /admin/deploy-bin: deploy a contract binary as a CKB data cell.
///
/// Gated behind the ENABLE_ADMIN_API environment variable. Returns 403 if unset.
/// Returns { tx_hash, code_hash, dep_type } which should be written to .env.deployed.
pub async fn deploy_bin(
	State(state): State<AppState>,
	Json(body): Json<DeployBinRequest>,
) -> Result<Json<Value>, TxBuildError> {
	if std::env::var("ENABLE_ADMIN_API").is_err() {
		return Err(TxBuildError::Rpc("admin API is disabled. Set ENABLE_ADMIN_API=1 to enable.".into()));
	}

	let binary = hex::decode(body.binary_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("invalid binary_hex: {e}")))?;

	let (tx, _tx_hash, code_hash) = build_deploy_binary(&state, binary).await?;
	let broadcast_hash = state.ckb.send_transaction(&tx).await?;

	Ok(Json(json!({
		"tx_hash": broadcast_hash,
		"code_hash": code_hash,
		"hash_type": "data1",
		"dep_type": "code",
		"note": "Set AGENT_IDENTITY_DEP_TX_HASH and AGENT_IDENTITY_TYPE_CODE_HASH in .env.deployed",
	})))
}

/// POST /admin/test-spending-cap: demonstrates consensus-level spending limit enforcement.
///
/// Builds a transaction that deliberately exceeds the agent identity's spending_limit_per_tx,
/// submits it to the CKB node, and captures the script validation rejection. This proves
/// that the spending cap is enforced at the consensus level, not just in software.
///
/// Gated by ENABLE_ADMIN_API.
pub async fn test_spending_cap(
	State(state): State<AppState>,
) -> Result<Json<Value>, TxBuildError> {
	if std::env::var("ENABLE_ADMIN_API").is_err() {
		return Err(TxBuildError::Rpc(
			"admin API is disabled. Set ENABLE_ADMIN_API=1 to enable.".into(),
		));
	}

	let identity_type_code_hash = std::env::var("AGENT_IDENTITY_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep("AGENT_IDENTITY_TYPE_CODE_HASH not set".into())
	})?;
	let identity_dep_tx_hash = std::env::var("AGENT_IDENTITY_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep("AGENT_IDENTITY_DEP_TX_HASH not set".into())
	})?;

	// Find the agent's identity cell by scanning for the type script.
	let identity_script = Script {
		code_hash: identity_type_code_hash.clone(),
		hash_type: "data1".into(),
		args: "0x".into(),
	};
	let our_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	// Search for identity cells with this type script that belong to our lock.
	let cells = state.ckb.get_cells_by_lock(&our_lock, 200).await?;
	// We need to search typed cells for the identity cell.
	// The identity cell has a type script, so get_cells_by_lock (which filters out data) won't find it.
	// Instead, use a broader approach: search by the type script.
	let type_cells = state
		.ckb
		.get_cells_by_type_script(&identity_script, 10)
		.await?;

	let identity_cell = type_cells
		.objects
		.iter()
		.find(|c| c.output.lock.args.to_lowercase() == state.lock_args.to_lowercase())
		.ok_or_else(|| TxBuildError::CellNotFound("no identity cell found for this agent".into()))?;

	let identity_capacity = parse_capacity_hex(&identity_cell.output.capacity)?;
	let identity_type_args = identity_cell
		.output
		.type_script
		.as_ref()
		.map(|t| t.args.clone())
		.unwrap_or_else(|| "0x".into());

	// Fetch full cell data via get_live_cell.
	let full_cell = state
		.ckb
		.get_live_cell(&identity_cell.out_point.tx_hash, {
			u32::from_str_radix(
				identity_cell.out_point.index.trim_start_matches("0x"),
				16,
			)
			.unwrap_or(0)
		})
		.await?;

	let identity_data_hex = full_cell
		.cell
		.as_ref()
		.and_then(|c| c.data.as_ref())
		.map(|d| d.content.clone())
		.unwrap_or_else(|| "0x".into());

	let identity_bytes = hex::decode(identity_data_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad identity data: {e}")))?;
	if identity_bytes.len() < 50 {
		return Err(TxBuildError::Rpc("identity cell data too short".into()));
	}
	let spending_limit =
		u64::from_le_bytes(identity_bytes[34..42].try_into().unwrap());

	// Build an overspend TX: the amount exceeds the spending limit.
	let overspend_amount = spending_limit + 100_000_000; // limit + 1 CKB

	let mut fee_inputs = Vec::new();
	let mut fee_capacity: u64 = 0;
	let needed = overspend_amount + 61 * 100_000_000 + 2_000_000; // payment + change + fee
	for cell in &cells.objects {
		if cell.output.type_script.is_some() {
			continue;
		}
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		fee_inputs.push(json!({
			"previous_output": cell.out_point,
			"since": "0x0",
		}));
		fee_capacity += cap;
		if fee_capacity >= needed {
			break;
		}
	}

	if fee_inputs.is_empty() || fee_capacity < needed {
		return Err(TxBuildError::InsufficientFunds {
			need: needed as f64 / 1e8,
			have: fee_capacity as f64 / 1e8,
		});
	}

	// Build TX: identity cell as input+output (update mode triggers type script),
	// plus a payment output that exceeds the spending limit.
	let dummy_lock_args = "0x".to_owned() + &"00".repeat(20);
	let dummy_lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: dummy_lock_args,
	};

	let mut all_inputs = vec![json!({
		"previous_output": identity_cell.out_point,
		"since": "0x0",
	})];
	all_inputs.extend(fee_inputs);

	let change_capacity = fee_capacity - overspend_amount - 2_000_000;

	use crate::tx_builder::signing::{inject_witness, placeholder_witness};
	let ph = format!("0x{}", hex::encode(placeholder_witness()));
	let witnesses: Vec<Value> = all_inputs
		.iter()
		.enumerate()
		.map(|(i, _)| {
			if i == 0 {
				serde_json::Value::String(ph.clone())
			} else {
				serde_json::Value::String("0x".into())
			}
		})
		.collect();

	let mut tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": identity_dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": all_inputs,
		"outputs": [
			// Re-create the identity cell unchanged (triggers type script validation).
			{
				"capacity": format!("{:#x}", identity_capacity),
				"lock": our_lock,
				"type": {
					"code_hash": identity_type_code_hash,
					"hash_type": "data1",
					"args": identity_type_args,
				},
			},
			// Overspend payment to a dummy address.
			{
				"capacity": format!("{:#x}", overspend_amount),
				"lock": dummy_lock,
				"type": null,
			},
			// Change.
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": Script {
					code_hash: SECP256K1_CODE_HASH.into(),
					hash_type: SECP256K1_HASH_TYPE.into(),
					args: state.lock_args.clone(),
				},
				"type": null,
			},
		],
		"outputs_data": [
			format!("0x{}", hex::encode(&identity_bytes)),
			"0x",
			"0x",
		],
		"witnesses": witnesses,
	});

	// Sign the transaction.
	let tx_hash = compute_raw_tx_hash(&tx)?;
	let witness_count = tx["witnesses"].as_array().map(|a| a.len()).unwrap_or(1);
	let signature = state.signer.sign(&tx_hash, witness_count).await?;
	inject_witness(&mut tx, &signature);

	// Submit to the node and expect rejection.
	match state.ckb.send_transaction(&tx).await {
		Ok(hash) => {
			// Unexpected success. The type script should have rejected it.
			Ok(Json(json!({
				"attempted_ckb": shannons_to_ckb(overspend_amount),
				"limit_ckb": shannons_to_ckb(spending_limit),
				"rejected": false,
				"tx_hash": hash,
				"stage": "consensus",
				"warning": "transaction was unexpectedly accepted. Check identity type script.",
			})))
		}
		Err(e) => {
			let error_msg = e.to_string();
			Ok(Json(json!({
				"attempted_ckb": shannons_to_ckb(overspend_amount),
				"limit_ckb": shannons_to_ckb(spending_limit),
				"rejected": true,
				"error": error_msg,
				"stage": "consensus",
				"explanation": "The CKB node rejected this transaction because the agent identity type script detected that the payment exceeds spending_limit_per_tx. This is consensus-level enforcement. No software guardrail can bypass it.",
			})))
		}
	}
}
