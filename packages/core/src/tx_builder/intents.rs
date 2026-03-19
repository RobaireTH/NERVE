use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{errors::TxBuildError, state::ckb_to_shannons, AppState};

use super::{
	badge::build_mint_badge,
	capability::{build_mint_capability, build_mint_reputation_capability},
	identity::{build_spawn_agent, build_spawn_sub_agent},
	job::{
		build_cancel_job, build_claim_job, build_complete_job, build_post_job,
		build_reserve_job, parse_hash_32, parse_lock_args_20,
	},
	reputation::{
		build_create_reputation, build_finalize_reputation,
		build_propose_reputation, compute_settlement_hash,
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
		/// Optional UTF-8 task description stored on-chain in the job cell data.
		description: Option<String>,
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
		/// Raw UTF-8 result text. The builder computes the binding hash and proof witness.
		result: Option<String>,
	},
	/// Cancel an Open/Reserved job: destroy the cell and reclaim capacity to poster.
	CancelJob {
		job_tx_hash: String,
		job_index: u32,
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
	/// Spawn a sub-agent with its own on-chain identity, linked to this agent as parent.
	SpawnSubAgent {
		spending_limit_ckb: f64,
		daily_limit_ckb: f64,
		/// Revenue share in basis points (0-10000). 1000 = 10%.
		revenue_share_bps: u16,
		/// Optional initial CKB funding for the sub-agent (default 100 CKB).
		initial_funding_ckb: Option<f64>,
	},
	/// Create a new reputation cell for this agent.
	CreateReputation,
	/// Propose a reputation update with settlement hash evidence (Idle → Proposed).
	ProposeReputation {
		rep_tx_hash: String,
		rep_index: u32,
		/// 1 = completed, 2 = abandoned.
		propose_type: u8,
		/// Dispute window in blocks (default: 100).
		dispute_window_blocks: Option<u64>,
		/// Job cell tx_hash for settlement hash computation.
		job_tx_hash: String,
		/// Job cell output index.
		job_index: u32,
		/// Worker's lock_args (0x-prefixed 20-byte hex).
		worker_lock_args: String,
		/// Poster's lock_args (0x-prefixed 20-byte hex).
		poster_lock_args: String,
		/// Reward amount in shannons.
		reward_shannons: u64,
		/// Optional result hash (0x-prefixed 32-byte hex).
		result_hash: Option<String>,
	},
	/// Finalize a proposed reputation update (Proposed → Finalized).
	FinalizeReputation {
		rep_tx_hash: String,
		rep_index: u32,
	},
	/// Mint a capability NFT backed by reputation chain evidence.
	MintReputationCapability {
		/// blake2b-256 hash of the capability type (0x-prefixed hex).
		capability_hash: String,
		/// Current proof_root from the agent's reputation cell (0x-prefixed 32-byte hex).
		reputation_proof_root: String,
		/// Settlement hash used as evidence (0x-prefixed 32-byte hex).
		settlement_hash: String,
	},
}

#[derive(Debug, Serialize)]
pub struct BuildResult {
	pub tx_hash: String,
	pub tx: Value,
	/// Optional metadata for intent-specific return values (e.g., sub-agent lock_args).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub metadata: Option<Value>,
}

pub async fn build_and_sign(
	state: &AppState,
	req: BuildRequest,
) -> Result<BuildResult, TxBuildError> {
	match req {
		BuildRequest::Transfer { to_lock_args, amount_ckb } => {
			let (tx, tx_hash) =
				build_transfer(state, &to_lock_args, ckb_to_shannons(amount_ckb)).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
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
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::SpawnSubAgent {
			spending_limit_ckb,
			daily_limit_ckb,
			revenue_share_bps,
			initial_funding_ckb,
		} => {
			if revenue_share_bps > 10000 {
				return Err(TxBuildError::SubAgentError(
					"revenue_share_bps must be 0-10000".into(),
				));
			}

			// Generate a fresh keypair for the sub-agent.
			let secp = secp256k1::Secp256k1::new();
			let (child_sk, child_pk) = secp.generate_keypair(&mut rand::rngs::OsRng);
			let child_pubkey = child_pk.serialize();
			let child_private_key = child_sk.secret_bytes().to_vec();
			let child_lock_args = crate::state::derive_lock_args(&child_private_key)?;

			let parent_lock_args_bytes = parse_lock_args_20(&state.lock_args)?;
			let funding_shannons = ckb_to_shannons(initial_funding_ckb.unwrap_or(100.0));

			let (tx, tx_hash) = build_spawn_sub_agent(
				state,
				&child_pubkey,
				&child_lock_args,
				ckb_to_shannons(spending_limit_ckb),
				ckb_to_shannons(daily_limit_ckb),
				&parent_lock_args_bytes,
				revenue_share_bps,
				funding_shannons,
			)
			.await?;

			// Register the sub-agent key in state for future signing.
			state
				.register_sub_agent(crate::state::SubAgentInfo {
					private_key_hex: format!("0x{}", hex::encode(&child_private_key)),
					lock_args: child_lock_args.clone(),
					parent_lock_args: state.lock_args.clone(),
					revenue_share_bps,
					identity_outpoint: Some(format!("{tx_hash}:0")),
				})
				.await?;

			let metadata = serde_json::json!({
				"sub_agent_lock_args": child_lock_args,
				"revenue_share_bps": revenue_share_bps,
				"initial_funding_ckb": initial_funding_ckb.unwrap_or(100.0),
			});

			Ok(BuildResult {
				tx_hash,
				tx,
				metadata: Some(metadata),
			})
		}

		BuildRequest::PostJob { reward_ckb, ttl_blocks, capability_hash, description } => {
			let cap_hash = parse_hash_32(&capability_hash)?;
			let (tx, tx_hash) =
				build_post_job(state, ckb_to_shannons(reward_ckb), ttl_blocks, cap_hash, description).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::ReserveJob { job_tx_hash, job_index, worker_lock_args } => {
			let (tx, tx_hash) =
				build_reserve_job(state, &job_tx_hash, job_index, &worker_lock_args).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::ClaimJob { job_tx_hash, job_index } => {
			let (tx, tx_hash) = build_claim_job(state, &job_tx_hash, job_index).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::CompleteJob { job_tx_hash, job_index, worker_lock_args, result } => {
			let (tx, tx_hash) =
				build_complete_job(state, &job_tx_hash, job_index, &worker_lock_args, result).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::CancelJob { job_tx_hash, job_index } => {
			let (tx, tx_hash) = build_cancel_job(state, &job_tx_hash, job_index).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::MintCapability { capability_hash } => {
			let cap_hash = parse_hash_32(&capability_hash)?;
			let (tx, tx_hash) = build_mint_capability(state, &cap_hash).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
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
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::CreateReputation => {
			let (tx, tx_hash) = build_create_reputation(state).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::ProposeReputation {
			rep_tx_hash,
			rep_index,
			propose_type,
			dispute_window_blocks,
			job_tx_hash,
			job_index,
			worker_lock_args,
			poster_lock_args,
			reward_shannons,
			result_hash,
		} => {
			let job_tx = parse_hash_32(&job_tx_hash)?;
			let worker = parse_lock_args_20(&worker_lock_args)?;
			let poster = parse_lock_args_20(&poster_lock_args)?;
			let rh = result_hash.as_deref().map(parse_hash_32).transpose()?;

			let settlement = compute_settlement_hash(
				&job_tx,
				job_index,
				&worker,
				&poster,
				reward_shannons,
				rh.as_ref(),
			);

			let (tx, tx_hash) = build_propose_reputation(
				state,
				&rep_tx_hash,
				rep_index,
				propose_type,
				dispute_window_blocks.unwrap_or(100),
				settlement,
			)
			.await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::FinalizeReputation { rep_tx_hash, rep_index } => {
			let (tx, tx_hash) =
				build_finalize_reputation(state, &rep_tx_hash, rep_index).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
		}

		BuildRequest::MintReputationCapability {
			capability_hash,
			reputation_proof_root,
			settlement_hash,
		} => {
			let cap_hash = parse_hash_32(&capability_hash)?;
			let proof_root = parse_hash_32(&reputation_proof_root)?;
			let settlement = parse_hash_32(&settlement_hash)?;
			let (tx, tx_hash) =
				build_mint_reputation_capability(state, &cap_hash, &proof_root, &settlement).await?;
			Ok(BuildResult { tx_hash, tx, metadata: None })
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
