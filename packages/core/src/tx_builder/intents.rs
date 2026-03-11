use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{errors::TxBuildError, state::ckb_to_shannons, AppState};

use super::{
	identity::build_spawn_agent,
	job::{build_post_job, parse_hash_32},
	transfer::build_transfer,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum BuildRequest {
	/// Simple CKB transfer to another address.
	Transfer {
		/// Lock args (0x-prefixed 20-byte hex) of the recipient's secp256k1-blake2b cell.
		to_lock_args: String,
		/// Amount to send in CKB.
		amount_ckb: f64,
	},
	/// Deploy an agent identity cell for this agent (requires AGENT_IDENTITY_TYPE_CODE_HASH).
	SpawnAgent {
		/// Per-transaction spending cap in CKB.
		spending_limit_ckb: f64,
		/// Daily spending cap in CKB.
		daily_limit_ckb: f64,
	},
	/// Post a new job cell with a CKB reward locked inside.
	PostJob {
		/// CKB reward paid to the worker on completion.
		reward_ckb: f64,
		/// Number of blocks before the job expires and the poster can reclaim.
		ttl_blocks: u64,
		/// blake2b-256 hash (0x-prefixed hex) of the required capability type.
		capability_hash: String,
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

		BuildRequest::SpawnAgent { spending_limit_ckb, daily_limit_ckb } => {
			let pubkey = derive_compressed_pubkey(&state.private_key)?;
			let spending_limit_shannons = ckb_to_shannons(spending_limit_ckb);
			let daily_limit_shannons = ckb_to_shannons(daily_limit_ckb);
			let (tx, tx_hash) = build_spawn_agent(
				state,
				&pubkey,
				spending_limit_shannons,
				daily_limit_shannons,
			)
			.await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::PostJob { reward_ckb, ttl_blocks, capability_hash } => {
			let reward_shannons = ckb_to_shannons(reward_ckb);
			let cap_hash = parse_hash_32(&capability_hash)?;
			let (tx, tx_hash) = build_post_job(state, reward_shannons, ttl_blocks, cap_hash).await?;
			Ok(BuildResult { tx_hash, tx })
		}
	}
}

fn derive_compressed_pubkey(private_key: &[u8]) -> Result<[u8; 33], TxBuildError> {
	use secp256k1::{PublicKey, Secp256k1, SecretKey};
	let secp = Secp256k1::new();
	let sk = SecretKey::from_slice(private_key)
		.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;
	let pk = PublicKey::from_secret_key(&secp, &sk);
	Ok(pk.serialize())
}
