use blake2b_rs::Blake2bBuilder;
use serde_json::Value;

use crate::errors::TxBuildError;

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

pub fn placeholder_witness_with_input_type(input_type: &[u8]) -> Vec<u8> {
	let total: u32 = 89 + input_type.len() as u32;
	let offset_lock: u32 = 16;
	let offset_input_type: u32 = 85;
	let offset_output_type: u32 = total;
	let lock_len: u32 = 65;
	let input_type_len: u32 = input_type.len() as u32;

	let mut w = Vec::with_capacity(total as usize);
	w.extend_from_slice(&total.to_le_bytes());
	w.extend_from_slice(&offset_lock.to_le_bytes());
	w.extend_from_slice(&offset_input_type.to_le_bytes());
	w.extend_from_slice(&offset_output_type.to_le_bytes());
	w.extend_from_slice(&lock_len.to_le_bytes());
	w.extend_from_slice(&[0u8; 65]);
	w.extend_from_slice(&input_type_len.to_le_bytes());
	w.extend_from_slice(input_type);
	w
}

pub fn signed_witness(signature: &[u8; 65]) -> Vec<u8> {
	let mut w = placeholder_witness();
	// Signature starts at byte 20 (4 total + 12 offsets + 4 lock_len).
	w[20..85].copy_from_slice(signature);
	w
}

pub fn compute_signing_message(
	tx_hash_hex: &str,
	witness_placeholder: &[u8],
	additional_witnesses: &[Vec<u8>],
) -> Result<[u8; 32], TxBuildError> {
	let tx_hash = hex::decode(tx_hash_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Signing(format!("bad tx hash: {e}")))?;
	if tx_hash.len() != 32 {
		return Err(TxBuildError::Signing("tx hash must be 32 bytes".into()));
	}

	// CKB sighash_all signing message: blake2b("ckb-default-hash" personalization)
	//   over: tx_hash(32) || len(witness_0)(8) || witness_0 || len(witness_1)(8) || witness_1 || ...
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();

	hasher.update(&tx_hash);

	let witness_len = witness_placeholder.len() as u64;
	hasher.update(&witness_len.to_le_bytes());
	hasher.update(witness_placeholder);

	for witness in additional_witnesses {
		let len = witness.len() as u64;
		hasher.update(&len.to_le_bytes());
		hasher.update(witness);
	}

	let mut result = [0u8; 32];
	hasher.finalize(&mut result);
	Ok(result)
}

#[cfg(test)]
fn sign_tx(
	tx_hash_hex: &str,
	private_key: &[u8],
	witness_count: usize,
) -> Result<[u8; 65], TxBuildError> {
	let placeholder = placeholder_witness();
	let additional: Vec<Vec<u8>> = (1..witness_count).map(|_| Vec::new()).collect();
	let message_bytes = compute_signing_message(tx_hash_hex, &placeholder, &additional)?;

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

#[cfg(test)]
fn sign_tx_with_witness(
	tx_hash_hex: &str,
	private_key: &[u8],
	first_witness: &[u8],
	witness_count: usize,
) -> Result<[u8; 65], TxBuildError> {
	let additional: Vec<Vec<u8>> = (1..witness_count).map(|_| Vec::new()).collect();
	let message_bytes = compute_signing_message(tx_hash_hex, first_witness, &additional)?;

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

pub fn inject_witness(tx: &mut Value, signature: &[u8; 65]) {
	if let Some(witnesses) = tx["witnesses"].as_array_mut() {
		if witnesses.is_empty() {
			let witness_hex = format!("0x{}", hex::encode(signed_witness(signature)));
			witnesses.push(serde_json::Value::String(witness_hex));
		} else {
			let existing = witnesses[0].as_str().unwrap_or("0x");
			let mut bytes = hex::decode(existing.trim_start_matches("0x")).unwrap_or_default();
			if bytes.len() >= 85 {
				bytes[20..85].copy_from_slice(signature);
			} else {
				bytes = signed_witness(signature);
			}
			witnesses[0] = serde_json::Value::String(format!("0x{}", hex::encode(&bytes)));
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use secp256k1::{Message, Secp256k1, SecretKey};

	#[test]
	fn placeholder_witness_layout() {
		let w = placeholder_witness();
		assert_eq!(w.len(), 85, "placeholder witness must be 85 bytes");

		// Total size field.
		let total = u32::from_le_bytes(w[0..4].try_into().unwrap());
		assert_eq!(total, 85);

		// Offset fields.
		let offset_lock = u32::from_le_bytes(w[4..8].try_into().unwrap());
		assert_eq!(offset_lock, 16);
		let offset_input_type = u32::from_le_bytes(w[8..12].try_into().unwrap());
		assert_eq!(offset_input_type, 85);
		let offset_output_type = u32::from_le_bytes(w[12..16].try_into().unwrap());
		assert_eq!(offset_output_type, 85);

		// Lock field length.
		let lock_len = u32::from_le_bytes(w[16..20].try_into().unwrap());
		assert_eq!(lock_len, 65);

		// Lock field content (all zeros for placeholder).
		assert!(w[20..85].iter().all(|&b| b == 0));
	}

	#[test]
	fn signed_witness_places_signature() {
		let mut sig = [0u8; 65];
		sig[0] = 0xAA;
		sig[64] = 0xBB;

		let w = signed_witness(&sig);
		assert_eq!(w.len(), 85);
		assert_eq!(w[20], 0xAA, "first sig byte at offset 20");
		assert_eq!(w[84], 0xBB, "last sig byte at offset 84");
	}

	#[test]
	fn signing_message_single_witness() {
		let tx_hash = "0x" .to_owned() + &"ab".repeat(32);
		let placeholder = placeholder_witness();
		let msg = compute_signing_message(&tx_hash, &placeholder, &[]).unwrap();
		// Should produce a deterministic 32-byte hash.
		assert_eq!(msg.len(), 32);
		// Verify the hash changes with a different tx_hash.
		let tx_hash2 = "0x".to_owned() + &"cd".repeat(32);
		let msg2 = compute_signing_message(&tx_hash2, &placeholder, &[]).unwrap();
		assert_ne!(msg, msg2);
	}

	#[test]
	fn signing_message_includes_additional_witnesses() {
		let tx_hash = "0x".to_owned() + &"ab".repeat(32);
		let placeholder = placeholder_witness();

		let msg_1 = compute_signing_message(&tx_hash, &placeholder, &[]).unwrap();
		// With one empty additional witness, the hash must differ.
		let msg_2 = compute_signing_message(&tx_hash, &placeholder, &[vec![]]).unwrap();
		assert_ne!(msg_1, msg_2, "additional empty witness must change the signing message");

		// With two empty witnesses.
		let msg_3 = compute_signing_message(&tx_hash, &placeholder, &[vec![], vec![]]).unwrap();
		assert_ne!(msg_2, msg_3, "each additional witness must change the message");
	}

	#[test]
	fn sign_tx_produces_recoverable_signature() {
		// Known test private key (NOT a real key).
		let privkey = hex::decode("e79f3207ea4980b7fed79956d5934249ceac4751a4fae01a0f7c4a96884bc4e3").unwrap();
		let tx_hash = "0x".to_owned() + &"11".repeat(32);

		let sig = sign_tx(&tx_hash, &privkey, 1).unwrap();
		assert_eq!(sig.len(), 65);
		// Recovery ID should be 0 or 1.
		assert!(sig[64] <= 1, "recovery id must be 0 or 1, got {}", sig[64]);

		// Verify the signature can recover the public key.
		let secp = secp256k1::Secp256k1::new();
		let placeholder = placeholder_witness();
		let msg_bytes = compute_signing_message(&tx_hash, &placeholder, &[]).unwrap();
		let msg = secp256k1::Message::from_digest_slice(&msg_bytes).unwrap();
		let rid = secp256k1::ecdsa::RecoveryId::from_i32(sig[64] as i32).unwrap();
		let rec_sig = secp256k1::ecdsa::RecoverableSignature::from_compact(&sig[..64], rid).unwrap();
		let recovered_pk = secp.recover_ecdsa(&msg, &rec_sig).unwrap();

		// Derive expected public key from private key.
		let sk = secp256k1::SecretKey::from_slice(&privkey).unwrap();
		let expected_pk = secp256k1::PublicKey::from_secret_key(&secp, &sk);
		assert_eq!(recovered_pk, expected_pk);
	}

	#[test]
	fn placeholder_witness_with_input_type_layout() {
		let input_type = vec![0xAA; 10];
		let w = placeholder_witness_with_input_type(&input_type);
		assert_eq!(w.len(), 89 + 10, "total = 89 + input_type.len()");

		let total = u32::from_le_bytes(w[0..4].try_into().unwrap());
		assert_eq!(total, 99);

		let offset_lock = u32::from_le_bytes(w[4..8].try_into().unwrap());
		assert_eq!(offset_lock, 16);
		let offset_input_type = u32::from_le_bytes(w[8..12].try_into().unwrap());
		assert_eq!(offset_input_type, 85);
		let offset_output_type = u32::from_le_bytes(w[12..16].try_into().unwrap());
		assert_eq!(offset_output_type, 99);

		let lock_len = u32::from_le_bytes(w[16..20].try_into().unwrap());
		assert_eq!(lock_len, 65);
		assert!(w[20..85].iter().all(|&b| b == 0), "lock placeholder zeroed");

		let it_len = u32::from_le_bytes(w[85..89].try_into().unwrap());
		assert_eq!(it_len, 10);
		assert_eq!(&w[89..99], &[0xAA; 10]);
	}

	#[test]
	fn sign_tx_with_witness_uses_custom_witness() {
		let privkey = hex::decode("e79f3207ea4980b7fed79956d5934249ceac4751a4fae01a0f7c4a96884bc4e3").unwrap();
		let tx_hash = "0x".to_owned() + &"11".repeat(32);

		let custom_witness = placeholder_witness_with_input_type(b"test-data");
		let sig = sign_tx_with_witness(&tx_hash, &privkey, &custom_witness, 1).unwrap();
		assert_eq!(sig.len(), 65);

		// Signing message must differ from a plain placeholder.
		let plain_sig = sign_tx(&tx_hash, &privkey, 1).unwrap();
		assert_ne!(sig, plain_sig, "custom witness produces a different signature");
	}

	#[test]
	fn inject_witness_preserves_input_type() {
		let input_type = vec![0xBB; 10];
		let first_witness = placeholder_witness_with_input_type(&input_type);
		let first_hex = format!("0x{}", hex::encode(&first_witness));

		let sig = [0xFFu8; 65];
		let mut tx = serde_json::json!({ "witnesses": [first_hex] });
		inject_witness(&mut tx, &sig);

		let w = tx["witnesses"][0].as_str().unwrap();
		let decoded = hex::decode(w.trim_start_matches("0x")).unwrap();
		assert_eq!(decoded.len(), 99, "witness length preserved");
		assert_eq!(decoded[20], 0xFF, "signature injected");
		assert_eq!(&decoded[89..99], &[0xBB; 10], "input_type data preserved");
	}

	#[test]
	fn inject_witness_replaces_first() {
		let sig = [0xFFu8; 65];
		let mut tx = serde_json::json!({
			"witnesses": ["0x0000"]
		});
		inject_witness(&mut tx, &sig);
		let w = tx["witnesses"][0].as_str().unwrap();
		assert!(w.starts_with("0x"));
		// The signed witness should contain the signature bytes.
		let decoded = hex::decode(w.trim_start_matches("0x")).unwrap();
		assert_eq!(decoded.len(), 85);
		assert_eq!(decoded[20], 0xFF);
	}
}
