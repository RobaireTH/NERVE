use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{errors::TxBuildError, state::ckb_to_shannons, AppState};

use super::{
	badge::build_mint_badge,
	capability::build_mint_capability,
	identity::build_spawn_agent,
	job::{
		build_cancel_job, build_claim_job, build_complete_job, build_post_job,
		build_reserve_job, parse_hash_32,
	},
	reputation::{build_create_reputation, build_finalize_reputation, build_propose_reputation},
	swap::{build_create_pool, build_swap},
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
		/// Optional SHA-256 result hash (0x-prefixed 32-byte hex) for on-chain proof of work.
		result_hash: Option<String>,
	},
	/// Cancel an Open/Reserved job: destroy the cell and reclaim capacity to poster.
	CancelJob {
		job_tx_hash: String,
		job_index: u32,
	},
	/// Execute a CKB→TOKEN swap against the mock AMM pool.
	Swap {
		pool_tx_hash: String,
		pool_index: u32,
		amount_ckb: f64,
		/// Slippage tolerance in basis points (100 = 1%).
		slippage_bps: Option<u32>,
	},
	/// Create a new AMM pool with seed liquidity.
	CreatePool {
		seed_ckb: f64,
		seed_token_amount: u64,
	},
	/// Mint a capability NFT with a signed attestation proof.
	MintCapability {
		/// blake2b-256 hash of the capability type (0x-prefixed hex).
		capability_hash: String,
	},
	/// Mint a PoP (Proof of Participation) badge for a completed job.
	MintBadge {
		/// The job cell's original tx_hash (0x-prefixed 32-byte hex).
		job_tx_hash: String,
		/// The job cell's output index.
		job_index: u32,
		/// The worker's lock_args who completed the job (0x-prefixed 20-byte hex).
		worker_lock_args: String,
		/// Optional result hash from the completed work (0x-prefixed 32-byte hex).
		result_hash: Option<String>,
		/// The tx_hash of the complete_job transaction (0x-prefixed 32-byte hex).
		completed_at_tx: String,
	},
	/// Create a new reputation cell for this agent.
	CreateReputation,
	/// Propose a reputation update (Idle → Proposed).
	ProposeReputation {
		rep_tx_hash: String,
		rep_index: u32,
		/// 1 = completed, 2 = abandoned.
		propose_type: u8,
		/// Dispute window in blocks (default: 100).
		dispute_window_blocks: Option<u64>,
	},
	/// Finalize a proposed reputation update (Proposed → Finalized).
	FinalizeReputation {
		rep_tx_hash: String,
		rep_index: u32,
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

		BuildRequest::CompleteJob { job_tx_hash, job_index, worker_lock_args, result_hash } => {
			let parsed_hash = result_hash
				.as_deref()
				.map(parse_hash_32)
				.transpose()?;
			let (tx, tx_hash) =
				build_complete_job(state, &job_tx_hash, job_index, &worker_lock_args, parsed_hash).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::CancelJob { job_tx_hash, job_index } => {
			let (tx, tx_hash) = build_cancel_job(state, &job_tx_hash, job_index).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::Swap { pool_tx_hash, pool_index, amount_ckb, slippage_bps } => {
			let (tx, tx_hash) = build_swap(
				state,
				&pool_tx_hash,
				pool_index,
				ckb_to_shannons(amount_ckb),
				slippage_bps.unwrap_or(100),
			)
			.await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::CreatePool { seed_ckb, seed_token_amount } => {
			let (tx, tx_hash) =
				build_create_pool(state, ckb_to_shannons(seed_ckb), seed_token_amount as u128)
					.await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::MintCapability { capability_hash } => {
			let cap_hash = parse_hash_32(&capability_hash)?;
			let (tx, tx_hash) = build_mint_capability(state, &cap_hash).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::MintBadge {
			job_tx_hash,
			job_index,
			worker_lock_args,
			result_hash,
			completed_at_tx,
		} => {
			let (tx, tx_hash) = build_mint_badge(
				state,
				&job_tx_hash,
				job_index,
				&worker_lock_args,
				result_hash.as_deref(),
				&completed_at_tx,
			)
			.await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::CreateReputation => {
			let (tx, tx_hash) = build_create_reputation(state).await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::ProposeReputation {
			rep_tx_hash,
			rep_index,
			propose_type,
			dispute_window_blocks,
		} => {
			let (tx, tx_hash) = build_propose_reputation(
				state,
				&rep_tx_hash,
				rep_index,
				propose_type,
				dispute_window_blocks.unwrap_or(100),
			)
			.await?;
			Ok(BuildResult { tx_hash, tx })
		}

		BuildRequest::FinalizeReputation { rep_tx_hash, rep_index } => {
			let (tx, tx_hash) =
				build_finalize_reputation(state, &rep_tx_hash, rep_index).await?;
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
