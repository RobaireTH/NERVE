use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{errors::TxBuildError, state::ckb_to_shannons, AppState};

use super::transfer::build_transfer;

#[derive(Debug, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum BuildRequest {
	Transfer {
		/// Lock args (0x-prefixed 20-byte hex) of the recipient's secp256k1-blake2b cell.
		to_lock_args: String,
		/// Amount to send in CKB.
		amount_ckb: f64,
	},
}

#[derive(Debug, Serialize)]
pub struct BuildResult {
	pub tx_hash: String,
	pub tx: Value,
}

pub async fn build_and_sign(
	state: &AppState,
	req: BuildRequest,
) -> Result<BuildResult, TxBuildError> {
	match req {
		BuildRequest::Transfer { to_lock_args, amount_ckb } => {
			let amount_shannons = ckb_to_shannons(amount_ckb);
			let (tx, tx_hash) = build_transfer(state, &to_lock_args, amount_shannons).await?;
			Ok(BuildResult { tx_hash, tx })
		}
	}
}
