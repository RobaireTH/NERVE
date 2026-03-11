use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
	errors::TxBuildError,
	state::AppState,
	tx_builder::identity::build_deploy_binary,
};

#[derive(Debug, Deserialize)]
pub struct DeployBinRequest {
	/// Binary content as a 0x-prefixed hex string.
	pub binary_hex: String,
}

/// POST /admin/deploy-bin — deploy a contract binary as a CKB data cell.
///
/// Returns { tx_hash, code_hash, dep_type } which should be written to .env.deployed.
pub async fn deploy_bin(
	State(state): State<AppState>,
	Json(body): Json<DeployBinRequest>,
) -> Result<Json<Value>, TxBuildError> {
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
