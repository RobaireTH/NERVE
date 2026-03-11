use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{errors::TxBuildError, state::ckb_to_shannons, AppState};

use super::{
	identity::build_spawn_agent,
	job::{
		build_cancel_job, build_claim_job, build_complete_job, build_post_job,
		build_reserve_job, parse_hash_32,
	},
	transfer::build_transfer,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum BuildRequest {
	/// Simple CKB transfer to another address.
	Transfer {
		to_lock_args: String,
		amount_ckb: f64,
	},
	/// Deploy an agent identity cell for this agent.
	SpawnAgent {
		spending_limit_ckb: f64,
		daily_limit_ckb: f64,
	},
	/// Post a new job cell with a CKB reward locked inside.
	PostJob {
		reward_ckb: f64,
		ttl_blocks: u64,
		/// blake2b-256 hash (0x-prefixed hex) of the required capability type.
		capability_hash: String,
	},
	/// Transition an Open job cell → Reserved and set the worker's lock_args.
	ReserveJob {
		job_tx_hash: String,
		job_index: u32,
		worker_lock_args: String,
	},
	/// Transition a Reserved job cell → Claimed.
	ClaimJob {
		job_tx_hash: String,
		job_index: u32,
	},
	/// Settle a Claimed job: destroy the job cell and route reward to the worker.
	CompleteJob {
		job_tx_hash: String,
		job_index: u32,
		/// The worker's lock_args (0x-prefixed 20-byte hex) to receive the reward.
		worker_lock_args: String,
	},
	/// Cancel an Open/Reserved job: destroy the cell and reclaim capacity to poster.
	CancelJob {
		job_tx_hash: String,
		job_index: u32,
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
			let (tx, tx_hash) =
				build_transfer(state, &to_lock_args, ckb_to_shannons(amount_ckb)).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::SpawnAgent { spending_limit_ckb, daily_limit_ckb } => {
			let pubkey = derive_compressed_pubkey(&state.private_key)?;
			let (tx, tx_hash) = build_spawn_agent(
				state,
				&pubkey,
				ckb_to_shannons(spending_limit_ckb),
				ckb_to_shannons(daily_limit_ckb),
			)
			.await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::PostJob { reward_ckb, ttl_blocks, capability_hash } => {
			let cap_hash = parse_hash_32(&capability_hash)?;
			let (tx, tx_hash) =
				build_post_job(state, ckb_to_shannons(reward_ckb), ttl_blocks, cap_hash).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::ReserveJob { job_tx_hash, job_index, worker_lock_args } => {
			let (tx, tx_hash) =
				build_reserve_job(state, &job_tx_hash, job_index, &worker_lock_args).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::ClaimJob { job_tx_hash, job_index } => {
			let (tx, tx_hash) = build_claim_job(state, &job_tx_hash, job_index).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::CompleteJob { job_tx_hash, job_index, worker_lock_args } => {
			let (tx, tx_hash) =
				build_complete_job(state, &job_tx_hash, job_index, &worker_lock_args).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::CancelJob { job_tx_hash, job_index } => {
			let (tx, tx_hash) = build_cancel_job(state, &job_tx_hash, job_index).await?;
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
