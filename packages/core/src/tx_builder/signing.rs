use blake2b_rs::Blake2bBuilder;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde_json::Value;

use crate::errors::TxBuildError;

/// Builds the 85-byte molecule-encoded WitnessArgs with a 65-byte zero placeholder.
/// Layout: [total_size: u32LE][offset_lock: u32LE][offset_input_type: u32LE][offset_output_type: u32LE][lock_len: u32LE][65 zero bytes]
pub fn placeholder_witness() -> Vec<u8> {
	// WitnessArgs molecule Table with lock=Some([0u8;65]), input_type=None, output_type=None.
	let total: u32 = 85;
	let offset_lock: u32 = 16; // 4 (total) + 4*3 (offsets)
	let offset_input_type: u32 = 85; // lock occupies [16..85], so input_type at 85
	let offset_output_type: u32 = 85;
	let lock_len: u32 = 65;

	let mut w = Vec::with_capacity(85);
	w.extend_from_slice(&total.to_le_bytes());
	w.extend_from_slice(&offset_lock.to_le_bytes());
	w.extend_from_slice(&offset_input_type.to_le_bytes());
	w.extend_from_slice(&offset_output_type.to_le_bytes());
	w.extend_from_slice(&lock_len.to_le_bytes());
	w.extend_from_slice(&[0u8; 65]);
	w
}

/// Builds a signed WitnessArgs by writing the 65-byte signature into the lock field.
pub fn signed_witness(signature: &[u8; 65]) -> Vec<u8> {
	let mut w = placeholder_witness();
	// Signature starts at byte 20 (4 total + 12 offsets + 4 lock_len).
	w[20..85].copy_from_slice(signature);
	w
}

/// Computes the CKB signing message: blake2b(tx_hash || witness_len || witness_bytes).
pub fn compute_signing_message(
	tx_hash_hex: &str,
	witness_placeholder: &[u8],
) -> Result<[u8; 32], TxBuildError> {
	let tx_hash = hex::decode(tx_hash_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Signing(format!("bad tx hash: {e}")))?;
	if tx_hash.len() != 32 {
		return Err(TxBuildError::Signing("tx hash must be 32 bytes".into()));
	}

	// CKB signing message: blake2b("ckb-default-hash" personalization)
	//   over: tx_hash (32) || witness_length_as_u64le (8) || witness_bytes.
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();

	hasher.update(&tx_hash);
	let witness_len = witness_placeholder.len() as u64;
	hasher.update(&witness_len.to_le_bytes());
	hasher.update(witness_placeholder);

	let mut result = [0u8; 32];
	hasher.finalize(&mut result);
	Ok(result)
}

/// Signs a transaction: computes the signing message and returns the 65-byte compact + recovery signature.
pub fn sign_tx(
	tx_hash_hex: &str,
	private_key: &[u8],
) -> Result<[u8; 65], TxBuildError> {
	let placeholder = placeholder_witness();
	let message_bytes = compute_signing_message(tx_hash_hex, &placeholder)?;

	let secp = Secp256k1::new();
	let sk = SecretKey::from_slice(private_key)
		.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;

	let msg = Message::from_digest_slice(&message_bytes)
		.map_err(|e| TxBuildError::Signing(format!("bad message: {e}")))?;

	let (recovery_id, sig_bytes) = secp
		.sign_ecdsa_recoverable(&msg, &sk)
		.serialize_compact();

	let mut signature = [0u8; 65];
	signature[..64].copy_from_slice(&sig_bytes);
	signature[64] = recovery_id.to_i32() as u8;

	Ok(signature)
}

/// Injects a signed witness into a transaction JSON object (mutates witnesses[0]).
pub fn inject_witness(tx: &mut Value, signature: &[u8; 65]) {
	let witness_hex = format!("0x{}", hex::encode(signed_witness(signature)));
	if let Some(witnesses) = tx["witnesses"].as_array_mut() {
		if witnesses.is_empty() {
			witnesses.push(serde_json::Value::String(witness_hex));
		} else {
			witnesses[0] = serde_json::Value::String(witness_hex);
		}
	}
}
