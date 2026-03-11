use std::sync::Arc;

use blake2b_rs::Blake2bBuilder;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use crate::{ckb_client::CkbClient, errors::TxBuildError};

// Well-known secp256k1-blake2b lock script constants (same on mainnet and testnet).
pub const SECP256K1_CODE_HASH: &str =
	"0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
pub const SECP256K1_HASH_TYPE: &str = "type";

// The dep_group cell that bundles the secp256k1-blake2b lock script binary.
pub const SECP256K1_DEP_TX_HASH: &str =
	"0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c";

#[derive(Clone)]
pub struct AppState {
	pub ckb: Arc<CkbClient>,
	/// Raw private key bytes (kept in memory only, never logged or persisted).
	pub private_key: Vec<u8>,
	/// blake160(compressed_pubkey) — the lock args for this agent's identity cell.
	pub lock_args: String,
	/// Maximum CKB that a single transaction may transfer, in shannons.
	pub spending_limit_shannons: u64,
}

impl AppState {
	pub fn from_env() -> Result<Self, TxBuildError> {
		let rpc_url = std::env::var("CKB_RPC_URL")
			.unwrap_or_else(|_| "https://testnet.ckb.dev/rpc".into());
		let indexer_url = std::env::var("CKB_INDEXER_URL")
			.unwrap_or_else(|_| "https://testnet.ckb.dev/indexer".into());

		let private_key_hex = std::env::var("AGENT_PRIVATE_KEY")
			.map_err(|_| TxBuildError::Signing("AGENT_PRIVATE_KEY not set".into()))?;

		let private_key = hex::decode(private_key_hex.trim_start_matches("0x"))
			.map_err(|e| TxBuildError::Signing(format!("bad AGENT_PRIVATE_KEY hex: {e}")))?;

		let spending_limit_shannons: u64 = std::env::var("SPENDING_LIMIT_CKB")
			.ok()
			.and_then(|v| v.parse::<f64>().ok())
			.map(ckb_to_shannons)
			.unwrap_or(ckb_to_shannons(100.0)); // default: 100 CKB

		let lock_args = derive_lock_args(&private_key)?;

		Ok(Self {
			ckb: Arc::new(CkbClient::new(rpc_url, indexer_url)),
			private_key,
			lock_args,
			spending_limit_shannons,
		})
	}
}

/// Derives the secp256k1-blake2b lock args (blake160 of compressed pubkey).
pub fn derive_lock_args(private_key: &[u8]) -> Result<String, TxBuildError> {
	let secp = Secp256k1::new();
	let sk = SecretKey::from_slice(private_key)
		.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;
	let pk = PublicKey::from_secret_key(&secp, &sk);
	let compressed = pk.serialize(); // 33 bytes

	// CKB uses blake2b-256 with personalization "ckb-default-hash"; lock_args = first 20 bytes.
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(&compressed);
	let mut hash = [0u8; 32];
	hasher.finalize(&mut hash);

	Ok(format!("0x{}", hex::encode(&hash[..20])))
}

pub fn ckb_to_shannons(ckb: f64) -> u64 {
	(ckb * 1e8) as u64
}

pub fn shannons_to_ckb(shannons: u64) -> f64 {
	shannons as f64 / 1e8
}

pub fn parse_capacity_hex(hex_str: &str) -> Result<u64, TxBuildError> {
	crate::ckb_client::parse_hex_u64(hex_str)
}
