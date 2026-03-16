use std::sync::Arc;

use blake2b_rs::Blake2bBuilder;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use crate::{ckb_client::CkbClient, errors::TxBuildError};

// Well-known secp256k1-blake2b lock script constants (code_hash is the same on mainnet and testnet).
pub const SECP256K1_CODE_HASH: &str =
	"0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
pub const SECP256K1_HASH_TYPE: &str = "type";

// The dep_group cell that bundles the secp256k1-blake2b lock script binary.
// Testnet (Pudge): 0xf8de3bb47d055cdf460d93a2a6e1b05f7432f9777c8c474abf4eec1d4aee5d37
// Mainnet (Mirana): 0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c
pub const SECP256K1_DEP_TX_HASH: &str =
	"0xf8de3bb47d055cdf460d93a2a6e1b05f7432f9777c8c474abf4eec1d4aee5d37";

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

		let spending_limit_shannons: u64 = std::env::var("PER_TX_LIMIT_CKB")
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ckb_to_shannons_basic() {
		assert_eq!(ckb_to_shannons(1.0), 100_000_000);
		assert_eq!(ckb_to_shannons(100.0), 10_000_000_000);
		assert_eq!(ckb_to_shannons(0.01), 1_000_000);
	}

	#[test]
	fn shannons_to_ckb_basic() {
		assert!((shannons_to_ckb(100_000_000) - 1.0).abs() < f64::EPSILON);
		assert!((shannons_to_ckb(10_000_000_000) - 100.0).abs() < f64::EPSILON);
	}

	#[test]
	fn parse_capacity_hex_values() {
		assert_eq!(parse_capacity_hex("0x174876e800").unwrap(), 100_000_000_000);
		assert_eq!(parse_capacity_hex("0x0").unwrap(), 0);
		assert_eq!(parse_capacity_hex("0xff").unwrap(), 255);
	}

	#[test]
	fn derive_lock_args_deterministic() {
		let privkey = hex::decode(
			"e79f3207ea4980b7fed79956d5934249ceac4751a4fae01a0f7c4a96884bc4e3",
		)
		.unwrap();
		let args1 = derive_lock_args(&privkey).unwrap();
		let args2 = derive_lock_args(&privkey).unwrap();
		assert_eq!(args1, args2);
		assert!(args1.starts_with("0x"));
		// blake160 = 20 bytes = 40 hex chars + "0x" prefix.
		assert_eq!(args1.len(), 42);
	}

	#[test]
	fn derive_lock_args_rejects_bad_key() {
		let bad_key = vec![0u8; 10];
		assert!(derive_lock_args(&bad_key).is_err());
	}
}
