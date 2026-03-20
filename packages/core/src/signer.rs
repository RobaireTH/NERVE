use crate::errors::TxBuildError;
use crate::tx_builder::signing::{compute_signing_message, placeholder_witness};
use async_trait::async_trait;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::json;

#[async_trait]
pub trait Signer: Send + Sync {
	/// Sign a CKB transaction. Returns 65-byte recoverable ECDSA signature.
	async fn sign(&self, tx_hash: &str, witness_count: usize) -> Result<[u8; 65], TxBuildError>;

	/// Sign with a custom first witness (for txs with input_type data).
	async fn sign_with_witness(
		&self,
		tx_hash: &str,
		first_witness: &[u8],
		witness_count: usize,
	) -> Result<[u8; 65], TxBuildError>;

	/// Sign an attestation message (raw ECDSA signature). Used for capability NFT proofs.
	async fn attest(&self, message: &[u8; 32]) -> Result<Vec<u8>, TxBuildError>;

	/// Return the compressed public key (33 bytes).
	async fn pubkey(&self) -> Result<[u8; 33], TxBuildError>;

	/// Return the lock_args for this signer.
	fn lock_args(&self) -> &str;
}

pub struct LocalSigner {
	private_key: Vec<u8>,
	lock_args: String,
}

impl LocalSigner {
	pub fn new(private_key: Vec<u8>) -> Result<Self, TxBuildError> {
		let lock_args = crate::state::derive_lock_args(&private_key)?;
		Ok(Self { private_key, lock_args })
	}
}

#[async_trait]
impl Signer for LocalSigner {
	async fn sign(&self, tx_hash: &str, witness_count: usize) -> Result<[u8; 65], TxBuildError> {
		let placeholder = placeholder_witness();
		let additional: Vec<Vec<u8>> = (1..witness_count).map(|_| Vec::new()).collect();
		let message_bytes = compute_signing_message(tx_hash, &placeholder, &additional)?;

		let secp = Secp256k1::new();
		let sk = SecretKey::from_slice(&self.private_key)
			.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;

		let msg = Message::from_digest_slice(&message_bytes)
			.map_err(|e| TxBuildError::Signing(format!("bad message: {e}")))?;

		let (recovery_id, sig_bytes) = secp.sign_ecdsa_recoverable(&msg, &sk).serialize_compact();

		let mut signature = [0u8; 65];
		signature[..64].copy_from_slice(&sig_bytes);
		signature[64] = recovery_id.to_i32() as u8;

		Ok(signature)
	}

	async fn sign_with_witness(
		&self,
		tx_hash: &str,
		first_witness: &[u8],
		witness_count: usize,
	) -> Result<[u8; 65], TxBuildError> {
		let additional: Vec<Vec<u8>> = (1..witness_count).map(|_| Vec::new()).collect();
		let message_bytes = compute_signing_message(tx_hash, first_witness, &additional)?;

		let secp = Secp256k1::new();
		let sk = SecretKey::from_slice(&self.private_key)
			.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;

		let msg = Message::from_digest_slice(&message_bytes)
			.map_err(|e| TxBuildError::Signing(format!("bad message: {e}")))?;

		let (recovery_id, sig_bytes) = secp.sign_ecdsa_recoverable(&msg, &sk).serialize_compact();

		let mut signature = [0u8; 65];
		signature[..64].copy_from_slice(&sig_bytes);
		signature[64] = recovery_id.to_i32() as u8;

		Ok(signature)
	}

	async fn attest(&self, message: &[u8; 32]) -> Result<Vec<u8>, TxBuildError> {
		let secp = Secp256k1::new();
		let sk = SecretKey::from_slice(&self.private_key)
			.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;

		let msg = Message::from_digest_slice(message)
			.map_err(|e| TxBuildError::Signing(format!("bad message: {e}")))?;

		let (recovery_id, sig_bytes) = secp.sign_ecdsa_recoverable(&msg, &sk).serialize_compact();

		let mut signature = vec![0u8; 65];
		signature[..64].copy_from_slice(&sig_bytes);
		signature[64] = recovery_id.to_i32() as u8;

		Ok(signature)
	}

	async fn pubkey(&self) -> Result<[u8; 33], TxBuildError> {
		use secp256k1::{PublicKey, Secp256k1};

		let secp = Secp256k1::new();
		let sk = SecretKey::from_slice(&self.private_key)
			.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;
		let pk = PublicKey::from_secret_key(&secp, &sk);
		Ok(pk.serialize())
	}

	fn lock_args(&self) -> &str {
		&self.lock_args
	}
}

pub struct SuperiseSigner {
	http_client: reqwest::Client,
	url: String,
	lock_args: String,
}

impl SuperiseSigner {
	pub async fn new(superise_url: &str) -> Result<Self, TxBuildError> {
		let http_client = reqwest::Client::new();

		let address = Self::call_address(&http_client, superise_url).await?;
		let lock_args = Self::derive_lock_args_from_address(&address)?;

		Ok(Self {
			http_client,
			url: superise_url.to_string(),
			lock_args,
		})
	}

	async fn call_address(client: &reqwest::Client, url: &str) -> Result<String, TxBuildError> {
		let payload = json!({
			"jsonrpc": "2.0",
			"id": "1",
			"method": "nervos.address",
			"params": []
		});

		let response = client
			.post(url)
			.json(&payload)
			.send()
			.await
			.map_err(|e| TxBuildError::Signing(format!("SupeRISE request failed: {e}")))?;

		let result: serde_json::Value = response
			.json()
			.await
			.map_err(|e| TxBuildError::Signing(format!("bad SupeRISE response: {e}")))?;

		result
			.get("result")
			.and_then(|r| r.as_str())
			.map(|s| s.to_string())
			.ok_or_else(|| TxBuildError::Signing("SupeRISE address() returned no result".into()))
	}

	fn derive_lock_args_from_address(address: &str) -> Result<String, TxBuildError> {
		if !address.starts_with("ckt1") && !address.starts_with("ckb1") {
			return Err(TxBuildError::Signing(format!(
				"invalid CKB address format: {address}"
			)));
		}

		decode_lock_args_from_bech32(address)
	}

	async fn call_sign_message(
		&self,
		message_hex: &str,
	) -> Result<[u8; 65], TxBuildError> {
		let payload = json!({
			"jsonrpc": "2.0",
			"id": "1",
			"method": "nervos.sign_message",
			"params": [message_hex]
		});

		let response = self
			.http_client
			.post(&self.url)
			.json(&payload)
			.send()
			.await
			.map_err(|e| TxBuildError::Signing(format!("SupeRISE sign_message failed: {e}")))?;

		let result: serde_json::Value = response
			.json()
			.await
			.map_err(|e| TxBuildError::Signing(format!("bad SupeRISE response: {e}")))?;

		let sig_hex = result
			.get("result")
			.and_then(|r| r.as_str())
			.ok_or_else(|| TxBuildError::Signing("SupeRISE sign_message returned no result".into()))?;

		let sig_bytes = hex::decode(sig_hex.trim_start_matches("0x"))
			.map_err(|e| TxBuildError::Signing(format!("bad signature hex from SupeRISE: {e}")))?;

		if sig_bytes.len() != 65 {
			return Err(TxBuildError::Signing(format!(
				"SupeRISE returned wrong signature length: {} bytes, expected 65",
				sig_bytes.len()
			)));
		}

		let mut signature = [0u8; 65];
		signature.copy_from_slice(&sig_bytes);
		Ok(signature)
	}
}

#[async_trait]
impl Signer for SuperiseSigner {
	async fn sign(&self, tx_hash: &str, witness_count: usize) -> Result<[u8; 65], TxBuildError> {
		let placeholder = placeholder_witness();
		let additional: Vec<Vec<u8>> = (1..witness_count).map(|_| Vec::new()).collect();
		let message_bytes = compute_signing_message(tx_hash, &placeholder, &additional)?;

		let message_hex = format!("0x{}", hex::encode(message_bytes));
		self.call_sign_message(&message_hex).await
	}

	async fn sign_with_witness(
		&self,
		tx_hash: &str,
		first_witness: &[u8],
		witness_count: usize,
	) -> Result<[u8; 65], TxBuildError> {
		let additional: Vec<Vec<u8>> = (1..witness_count).map(|_| Vec::new()).collect();
		let message_bytes = compute_signing_message(tx_hash, first_witness, &additional)?;

		let message_hex = format!("0x{}", hex::encode(message_bytes));
		self.call_sign_message(&message_hex).await
	}

	async fn attest(&self, message: &[u8; 32]) -> Result<Vec<u8>, TxBuildError> {
		let message_hex = format!("0x{}", hex::encode(message));
		self.call_sign_message(&message_hex).await.map(|sig| sig.to_vec())
	}

	async fn pubkey(&self) -> Result<[u8; 33], TxBuildError> {
		let payload = json!({
			"jsonrpc": "2.0",
			"id": "1",
			"method": "nervos.pubkey",
			"params": []
		});

		let response = self
			.http_client
			.post(&self.url)
			.json(&payload)
			.send()
			.await
			.map_err(|e| TxBuildError::Signing(format!("SupeRISE pubkey() failed: {e}")))?;

		let result: serde_json::Value = response
			.json()
			.await
			.map_err(|e| TxBuildError::Signing(format!("bad SupeRISE response: {e}")))?;

		let pubkey_hex = result
			.get("result")
			.and_then(|r| r.as_str())
			.ok_or_else(|| TxBuildError::Signing("SupeRISE pubkey() returned no result".into()))?;

		let pubkey_bytes = hex::decode(pubkey_hex.trim_start_matches("0x"))
			.map_err(|e| TxBuildError::Signing(format!("bad pubkey hex from SupeRISE: {e}")))?;

		if pubkey_bytes.len() != 33 {
			return Err(TxBuildError::Signing(format!(
				"SupeRISE returned wrong pubkey length: {} bytes, expected 33",
				pubkey_bytes.len()
			)));
		}

		let mut pubkey = [0u8; 33];
		pubkey.copy_from_slice(&pubkey_bytes);
		Ok(pubkey)
	}

	fn lock_args(&self) -> &str {
		&self.lock_args
	}
}

fn decode_lock_args_from_bech32(address: &str) -> Result<String, TxBuildError> {
	if !address.starts_with("ckt1") && !address.starts_with("ckb1") {
		return Err(TxBuildError::Signing("address must start with ckt1 or ckb1".into()));
	}

	let without_prefix = &address[5..];

	let decoded = bech32_decode(without_prefix)
		.map_err(|e| TxBuildError::Signing(format!("bech32 decode error: {e}")))?;

	if decoded.len() < 21 {
		return Err(TxBuildError::Signing(
			"decoded address data too short".into(),
		));
	}

	let lock_args = &decoded[1..21];

	Ok(format!("0x{}", hex::encode(lock_args)))
}

fn bech32_decode(input: &str) -> Result<Vec<u8>, String> {
	const CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

	let mut result = Vec::new();
	let mut acc: u32 = 0;
	let mut bits: u32 = 0;

	for ch in input.chars() {
		let d = CHARSET
			.iter()
			.position(|&c| c == ch as u8)
			.ok_or_else(|| format!("invalid bech32 character: {ch}"))?;

		acc = (acc << 5) | (d as u32);
		bits += 5;

		if bits >= 8 {
			bits -= 8;
			result.push(((acc >> bits) & 0xff) as u8);
			acc &= (1 << bits) - 1;
		}
	}

	if bits > 4 || acc != 0 {
		return Err("invalid bech32 padding".into());
	}

	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn local_signer_derives_lock_args() {
		let privkey = hex::decode("e79f3207ea4980b7fed79956d5934249ceac4751a4fae01a0f7c4a96884bc4e3")
			.unwrap();
		let signer = LocalSigner::new(privkey).unwrap();
		assert!(signer.lock_args().starts_with("0x"));
		assert_eq!(signer.lock_args().len(), 42);
	}
}
