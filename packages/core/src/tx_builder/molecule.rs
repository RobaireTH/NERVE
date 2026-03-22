//! Minimal CKB molecule serialization for computing raw transaction hashes.
//!
//! CKB tx_hash = blake2b("ckb-default-hash", serialize(RawTransaction)).
//! The RawTransaction excludes witnesses, so the hash is stable before signing.

use blake2b_rs::Blake2bBuilder;
use serde_json::Value;

use crate::errors::TxBuildError;

pub fn compute_raw_tx_hash(tx: &Value) -> Result<String, TxBuildError> {
	let raw_bytes = serialize_raw_transaction(tx)?;
	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(&raw_bytes);
	let mut hash = [0u8; 32];
	hasher.finalize(&mut hash);
	Ok(format!("0x{}", hex::encode(hash)))
}

/// Table: total_size(4 LE) + field_offsets(N * 4 LE) + field data.
fn serialize_table(fields: &[Vec<u8>]) -> Vec<u8> {
	let header_size = 4 + fields.len() * 4;
	let data_size: usize = fields.iter().map(|f| f.len()).sum();
	let total_size = header_size + data_size;

	let mut buf = Vec::with_capacity(total_size);
	buf.extend_from_slice(&(total_size as u32).to_le_bytes());

	let mut offset = header_size as u32;
	for field in fields {
		buf.extend_from_slice(&offset.to_le_bytes());
		offset += field.len() as u32;
	}

	for field in fields {
		buf.extend_from_slice(field);
	}

	buf
}

/// FixVec: item_count(4 LE) + items (each fixed-size).
fn serialize_fixvec(items: &[Vec<u8>]) -> Vec<u8> {
	let mut buf = Vec::new();
	buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
	for item in items {
		buf.extend_from_slice(item);
	}
	buf
}

/// DynVec: total_size(4 LE) + offsets(N * 4 LE) + items (variable-size).
fn serialize_dynvec(items: &[Vec<u8>]) -> Vec<u8> {
	if items.is_empty() {
		return 4u32.to_le_bytes().to_vec();
	}

	let header_size = 4 + items.len() * 4;
	let data_size: usize = items.iter().map(|i| i.len()).sum();
	let total_size = header_size + data_size;

	let mut buf = Vec::with_capacity(total_size);
	buf.extend_from_slice(&(total_size as u32).to_le_bytes());

	let mut offset = header_size as u32;
	for item in items {
		buf.extend_from_slice(&offset.to_le_bytes());
		offset += item.len() as u32;
	}

	for item in items {
		buf.extend_from_slice(item);
	}

	buf
}

/// Bytes (FixVec<byte>): length(4 LE) + raw bytes.
fn serialize_bytes(data: &[u8]) -> Vec<u8> {
	let mut buf = Vec::with_capacity(4 + data.len());
	buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
	buf.extend_from_slice(data);
	buf
}

/// RawTransaction = Table { version, cell_deps, header_deps, inputs, outputs, outputs_data }.
fn serialize_raw_transaction(tx: &Value) -> Result<Vec<u8>, TxBuildError> {
	let version = serialize_version(tx)?;
	let cell_deps = serialize_cell_dep_vec(tx)?;
	let header_deps = serialize_byte32_vec(tx)?;
	let inputs = serialize_cell_input_vec(tx)?;
	let outputs = serialize_cell_output_vec(tx)?;
	let outputs_data = serialize_bytes_vec(tx)?;

	Ok(serialize_table(&[
		version,
		cell_deps,
		header_deps,
		inputs,
		outputs,
		outputs_data,
	]))
}

fn serialize_version(tx: &Value) -> Result<Vec<u8>, TxBuildError> {
	let hex = tx["version"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("missing version".into()))?;
	Ok(parse_hex_u32(hex)?.to_le_bytes().to_vec())
}

fn serialize_cell_dep_vec(tx: &Value) -> Result<Vec<u8>, TxBuildError> {
	let deps = tx["cell_deps"]
		.as_array()
		.ok_or_else(|| TxBuildError::Rpc("missing cell_deps".into()))?;
	let mut items = Vec::new();
	for dep in deps {
		items.push(serialize_cell_dep(dep)?);
	}
	Ok(serialize_fixvec(&items))
}

/// CellDep = Struct { out_point(36), dep_type(1) } = 37 bytes.
fn serialize_cell_dep(dep: &Value) -> Result<Vec<u8>, TxBuildError> {
	let mut buf = serialize_out_point(&dep["out_point"])?;
	let dep_type = match dep["dep_type"].as_str() {
		Some("code") => 0u8,
		Some("dep_group") => 1u8,
		_ => return Err(TxBuildError::Rpc("invalid dep_type".into())),
	};
	buf.push(dep_type);
	Ok(buf)
}

/// OutPoint = Struct { tx_hash(32), index(4) } = 36 bytes.
fn serialize_out_point(op: &Value) -> Result<Vec<u8>, TxBuildError> {
	let tx_hash = hex_to_bytes32(
		op["tx_hash"]
			.as_str()
			.ok_or_else(|| TxBuildError::Rpc("missing out_point.tx_hash".into()))?,
	)?;
	let index = parse_hex_u32(
		op["index"]
			.as_str()
			.ok_or_else(|| TxBuildError::Rpc("missing out_point.index".into()))?,
	)?;
	let mut buf = Vec::with_capacity(36);
	buf.extend_from_slice(&tx_hash);
	buf.extend_from_slice(&index.to_le_bytes());
	Ok(buf)
}

fn serialize_byte32_vec(tx: &Value) -> Result<Vec<u8>, TxBuildError> {
	let deps = tx["header_deps"]
		.as_array()
		.ok_or_else(|| TxBuildError::Rpc("missing header_deps".into()))?;
	let mut items = Vec::new();
	for dep in deps {
		let hash = hex_to_bytes32(
			dep.as_str()
				.ok_or_else(|| TxBuildError::Rpc("invalid header_dep".into()))?,
		)?;
		items.push(hash.to_vec());
	}
	Ok(serialize_fixvec(&items))
}

fn serialize_cell_input_vec(tx: &Value) -> Result<Vec<u8>, TxBuildError> {
	let inputs = tx["inputs"]
		.as_array()
		.ok_or_else(|| TxBuildError::Rpc("missing inputs".into()))?;
	let mut items = Vec::new();
	for input in inputs {
		items.push(serialize_cell_input(input)?);
	}
	Ok(serialize_fixvec(&items))
}

/// CellInput = Struct { since(8), previous_output(36) } = 44 bytes.
fn serialize_cell_input(input: &Value) -> Result<Vec<u8>, TxBuildError> {
	let since = parse_hex_u64(
		input["since"]
			.as_str()
			.ok_or_else(|| TxBuildError::Rpc("missing input.since".into()))?,
	)?;
	let mut buf = Vec::with_capacity(44);
	buf.extend_from_slice(&since.to_le_bytes());
	buf.extend_from_slice(&serialize_out_point(&input["previous_output"])?);
	Ok(buf)
}

fn serialize_cell_output_vec(tx: &Value) -> Result<Vec<u8>, TxBuildError> {
	let outputs = tx["outputs"]
		.as_array()
		.ok_or_else(|| TxBuildError::Rpc("missing outputs".into()))?;
	let mut items = Vec::new();
	for output in outputs {
		items.push(serialize_cell_output(output)?);
	}
	Ok(serialize_dynvec(&items))
}

/// CellOutput = Table { capacity(Uint64), lock(Script), type_(ScriptOpt) }.
fn serialize_cell_output(output: &Value) -> Result<Vec<u8>, TxBuildError> {
	let capacity = parse_hex_u64(
		output["capacity"]
			.as_str()
			.ok_or_else(|| TxBuildError::Rpc("missing output.capacity".into()))?,
	)?;
	let capacity_bytes = capacity.to_le_bytes().to_vec();
	let lock_bytes = serialize_script(&output["lock"])?;
	let type_bytes = if output["type"].is_null() {
		Vec::new() // ScriptOpt::None = 0 bytes
	} else {
		serialize_script(&output["type"])?
	};
	Ok(serialize_table(&[capacity_bytes, lock_bytes, type_bytes]))
}

/// Script = Table { code_hash(Byte32), hash_type(byte), args(Bytes) }.
fn serialize_script(script: &Value) -> Result<Vec<u8>, TxBuildError> {
	let code_hash = hex_to_bytes32(
		script["code_hash"]
			.as_str()
			.ok_or_else(|| TxBuildError::Rpc("missing script.code_hash".into()))?,
	)?;
	let hash_type = match script["hash_type"].as_str() {
		Some("data") => 0u8,
		Some("type") => 1u8,
		Some("data1") => 2u8,
		Some("data2") => 4u8,
		_ => return Err(TxBuildError::Rpc("invalid hash_type".into())),
	};
	let args_hex = script["args"]
		.as_str()
		.ok_or_else(|| TxBuildError::Rpc("missing script.args".into()))?;
	let args = hex::decode(args_hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("bad script.args hex: {e}")))?;

	Ok(serialize_table(&[
		code_hash.to_vec(),
		vec![hash_type],
		serialize_bytes(&args),
	]))
}

fn serialize_bytes_vec(tx: &Value) -> Result<Vec<u8>, TxBuildError> {
	let data_array = tx["outputs_data"]
		.as_array()
		.ok_or_else(|| TxBuildError::Rpc("missing outputs_data".into()))?;
	let mut items = Vec::new();
	for data in data_array {
		let hex_str = data
			.as_str()
			.ok_or_else(|| TxBuildError::Rpc("invalid outputs_data entry".into()))?;
		let bytes = hex::decode(hex_str.trim_start_matches("0x"))
			.map_err(|e| TxBuildError::Rpc(format!("bad outputs_data hex: {e}")))?;
		items.push(serialize_bytes(&bytes));
	}
	Ok(serialize_dynvec(&items))
}

fn hex_to_bytes32(hex: &str) -> Result<[u8; 32], TxBuildError> {
	let bytes = hex::decode(hex.trim_start_matches("0x"))
		.map_err(|e| TxBuildError::Rpc(format!("hex decode: {e}")))?;
	if bytes.len() != 32 {
		return Err(TxBuildError::Rpc(format!(
			"expected 32 bytes, got {}",
			bytes.len()
		)));
	}
	let mut arr = [0u8; 32];
	arr.copy_from_slice(&bytes);
	Ok(arr)
}

fn parse_hex_u32(hex: &str) -> Result<u32, TxBuildError> {
	u32::from_str_radix(hex.trim_start_matches("0x"), 16)
		.map_err(|e| TxBuildError::Rpc(format!("hex u32 parse: {e}")))
}

fn parse_hex_u64(hex: &str) -> Result<u64, TxBuildError> {
	u64::from_str_radix(hex.trim_start_matches("0x"), 16)
		.map_err(|e| TxBuildError::Rpc(format!("hex u64 parse: {e}")))
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn table_layout() {
		let fields = vec![vec![1, 2], vec![3]];
		let result = serialize_table(&fields);
		// total_size = 4 + 2*4 + 2 + 1 = 15
		assert_eq!(u32::from_le_bytes(result[0..4].try_into().unwrap()), 15);
		assert_eq!(u32::from_le_bytes(result[4..8].try_into().unwrap()), 12);
		assert_eq!(u32::from_le_bytes(result[8..12].try_into().unwrap()), 14);
		assert_eq!(&result[12..14], &[1, 2]);
		assert_eq!(&result[14..15], &[3]);
	}

	#[test]
	fn fixvec_empty() {
		assert_eq!(serialize_fixvec(&[]), vec![0, 0, 0, 0]);
	}

	#[test]
	fn dynvec_empty() {
		assert_eq!(serialize_dynvec(&[]), vec![4, 0, 0, 0]);
	}

	#[test]
	fn bytes_basic() {
		assert_eq!(serialize_bytes(&[0xAA, 0xBB]), vec![2, 0, 0, 0, 0xAA, 0xBB]);
	}

	#[test]
	fn raw_tx_hash_deterministic() {
		let tx = json!({
			"version": "0x0",
			"cell_deps": [{
				"out_point": {
					"tx_hash": format!("0x{}", "ab".repeat(32)),
					"index": "0x0"
				},
				"dep_type": "dep_group"
			}],
			"header_deps": [],
			"inputs": [{
				"since": "0x0",
				"previous_output": {
					"tx_hash": format!("0x{}", "cd".repeat(32)),
					"index": "0x0"
				}
			}],
			"outputs": [{
				"capacity": "0x174876e800",
				"lock": {
					"code_hash": format!("0x{}", "ee".repeat(32)),
					"hash_type": "type",
					"args": "0x1234"
				},
				"type": null
			}],
			"outputs_data": ["0x"],
			"witnesses": ["0x"]
		});
		let hash1 = compute_raw_tx_hash(&tx).unwrap();
		let hash2 = compute_raw_tx_hash(&tx).unwrap();
		assert_eq!(hash1, hash2);
		assert!(hash1.starts_with("0x"));
		assert_eq!(hash1.len(), 66);
	}

	#[test]
	fn raw_tx_hash_ignores_witnesses() {
		let tx1 = json!({
			"version": "0x0",
			"cell_deps": [],
			"header_deps": [],
			"inputs": [{
				"since": "0x0",
				"previous_output": {
					"tx_hash": format!("0x{}", "ab".repeat(32)),
					"index": "0x0"
				}
			}],
			"outputs": [{
				"capacity": "0x174876e800",
				"lock": {
					"code_hash": format!("0x{}", "ee".repeat(32)),
					"hash_type": "type",
					"args": "0x"
				},
				"type": null
			}],
			"outputs_data": ["0x"],
			"witnesses": ["0x"]
		});
		let mut tx2 = tx1.clone();
		tx2["witnesses"] = json!(["0xdeadbeef"]);

		assert_eq!(
			compute_raw_tx_hash(&tx1).unwrap(),
			compute_raw_tx_hash(&tx2).unwrap(),
			"tx_hash must be independent of witnesses"
		);
	}

	#[test]
	fn raw_tx_hash_changes_with_outputs() {
		let base = json!({
			"version": "0x0",
			"cell_deps": [],
			"header_deps": [],
			"inputs": [{
				"since": "0x0",
				"previous_output": {
					"tx_hash": format!("0x{}", "ab".repeat(32)),
					"index": "0x0"
				}
			}],
			"outputs": [{
				"capacity": "0x174876e800",
				"lock": {
					"code_hash": format!("0x{}", "ee".repeat(32)),
					"hash_type": "type",
					"args": "0x"
				},
				"type": null
			}],
			"outputs_data": ["0x"],
			"witnesses": ["0x"]
		});
		let mut modified = base.clone();
		modified["outputs"][0]["capacity"] = json!("0x174876e801");

		assert_ne!(
			compute_raw_tx_hash(&base).unwrap(),
			compute_raw_tx_hash(&modified).unwrap(),
			"different capacity must produce different hash"
		);
	}
}
