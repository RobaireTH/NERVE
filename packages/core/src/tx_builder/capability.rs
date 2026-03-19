use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{parse_capacity_hex, AppState, SECP256K1_DEP_TX_HASH},
};

use super::molecule::compute_raw_tx_hash;
use super::signing::{inject_witness, sign_tx};

const ESTIMATED_FEE: u64 = 2_000_000;
// Minimum capacity for a capability NFT cell:
//   cap(8) + lock(53) + type(33) + data(54 + proof_bytes) ≈ 150+ bytes.
//   Use 200 CKB as safe minimum (covers ~200 bytes of proof data).
const CAP_NFT_CELL_MIN: u64 = 200 * 100_000_000;

fn cap_nft_type_env() -> Result<(String, String), TxBuildError> {
	let code_hash = std::env::var("CAP_NFT_TYPE_CODE_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"CAP_NFT_TYPE_CODE_HASH not set; run scripts/deploy_contracts.sh capability_nft first"
				.into(),
		)
	})?;
	let dep_tx_hash = std::env::var("CAP_NFT_DEP_TX_HASH").map_err(|_| {
		TxBuildError::MissingCellDep(
			"CAP_NFT_DEP_TX_HASH not set; run scripts/deploy_contracts.sh capability_nft first"
				.into(),
		)
	})?;
	Ok((code_hash, dep_tx_hash))
}

use super::{our_lock, placeholder_witnesses, MIN_CELL_CAPACITY};

/// Layout (54+ bytes):
///   [0]       version = 0
///   [1]       proof_type: 0=attestation
///   [2..22]   agent_lock_args: [u8; 20]
///   [22..54]  capability_hash: [u8; 32]
///   [54..]    proof_data (signed attestation bytes)
fn encode_capability_data(
	agent_lock_args: &[u8; 20],
	capability_hash: &[u8; 32],
	proof_data: &[u8],
) -> Vec<u8> {
	let mut data = Vec::with_capacity(54 + proof_data.len());
	data.push(0u8);
	data.push(0u8);
	data.extend_from_slice(agent_lock_args);
	data.extend_from_slice(capability_hash);
	data.extend_from_slice(proof_data);
	data
}

/// sign(blake2b("ckb-default-hash", agent_lock_args || capability_hash)).
fn create_attestation(
	private_key: &[u8],
	agent_lock_args: &[u8; 20],
	capability_hash: &[u8; 32],
) -> Result<Vec<u8>, TxBuildError> {
	use blake2b_rs::Blake2bBuilder;
	use secp256k1::{Message, Secp256k1, SecretKey};

	let mut hasher = Blake2bBuilder::new(32)
		.personal(b"ckb-default-hash")
		.build();
	hasher.update(agent_lock_args);
	hasher.update(capability_hash);
	let mut msg_bytes = [0u8; 32];
	hasher.finalize(&mut msg_bytes);

	let secp = Secp256k1::new();
	let sk = SecretKey::from_slice(private_key)
		.map_err(|e| TxBuildError::Signing(format!("invalid private key: {e}")))?;
	let msg = Message::from_digest_slice(&msg_bytes)
		.map_err(|e| TxBuildError::Signing(format!("bad message: {e}")))?;

	let (recovery_id, sig_bytes) = secp
		.sign_ecdsa_recoverable(&msg, &sk)
		.serialize_compact();

	let mut signature = vec![0u8; 65];
	signature[..64].copy_from_slice(&sig_bytes);
	signature[64] = recovery_id.to_i32() as u8;

	Ok(signature)
}

/// Layout (118 bytes, proof_type=1 reputation-chain-backed):
///   [0]       version = 0
///   [1]       proof_type = 1
///   [2..22]   agent_lock_args: [u8; 20]
///   [22..54]  capability_hash: [u8; 32]
///   [54..86]  proof_root_snapshot: [u8; 32]
///   [86..118] settlement_hash: [u8; 32]
fn encode_reputation_capability_data(
	agent_lock_args: &[u8; 20],
	capability_hash: &[u8; 32],
	proof_root: &[u8; 32],
	settlement_hash: &[u8; 32],
) -> Vec<u8> {
	let mut data = Vec::with_capacity(118);
	data.push(0u8);
	data.push(1u8);
	data.extend_from_slice(agent_lock_args);
	data.extend_from_slice(capability_hash);
	data.extend_from_slice(proof_root);
	data.extend_from_slice(settlement_hash);
	data
}

async fn find_reputation_cell_outpoint(
	state: &AppState,
	lock_args: &str,
) -> Result<Option<Value>, TxBuildError> {
	let type_code_hash = match std::env::var("REPUTATION_TYPE_CODE_HASH") {
		Ok(v) => v,
		Err(_) => return Ok(None),
	};

	let type_script = Script {
		code_hash: type_code_hash,
		hash_type: "data1".into(),
		args: "0x".into(),
	};

	let cells = state.ckb.get_cells_by_type_script(&type_script, 200).await?;

	for cell in &cells.objects {
		if cell.output.lock.args.to_lowercase() == lock_args.to_lowercase() {
			return Ok(Some(json!(cell.out_point)));
		}
	}

	Ok(None)
}

/// Includes the agent's reputation cell as a cell_dep so the type script can
/// cross-reference the proof_root_snapshot against the live on-chain proof_root.
pub async fn build_mint_reputation_capability(
	state: &AppState,
	capability_hash: &[u8; 32],
	proof_root: &[u8; 32],
	settlement_hash: &[u8; 32],
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = cap_nft_type_env()?;
	let agent_lock_args = super::job::parse_lock_args_20(&state.lock_args)?;

	let nft_data = encode_reputation_capability_data(&agent_lock_args, capability_hash, proof_root, settlement_hash);

	let occupied_bytes = 8 + 53 + 33 + nft_data.len() as u64;
	let nft_capacity = std::cmp::max(occupied_bytes * 100_000_000, CAP_NFT_CELL_MIN);

	let needed = nft_capacity + ESTIMATED_FEE + MIN_CELL_CAPACITY;
	let agent_lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&agent_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	for cell in &cells.objects {
		if cell.output.type_script.is_some() {
			continue;
		}
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		inputs.push(json!({ "previous_output": cell.out_point, "since": "0x0" }));
		input_capacity += cap;
		if input_capacity >= needed {
			break;
		}
	}
	if input_capacity < needed {
		return Err(TxBuildError::InsufficientFunds {
			need: needed as f64 / 1e8,
			have: input_capacity as f64 / 1e8,
		});
	}

	let change_capacity = input_capacity - nft_capacity - ESTIMATED_FEE;
	let witnesses = placeholder_witnesses(inputs.len());

	let mut cell_deps = vec![
		json!({ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" }),
		json!({ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" }),
	];

	if let Some(rep_outpoint) = find_reputation_cell_outpoint(state, &state.lock_args).await? {
		cell_deps.push(json!({ "out_point": rep_outpoint, "dep_type": "code" }));
	} else {
		return Err(TxBuildError::CellNotFound(
			"agent reputation cell not found; required for proof_type=1 capability NFT".into(),
		));
	}

	let tx = json!({
		"version": "0x0",
		"cell_deps": cell_deps,
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", nft_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(&nft_data)), "0x"],
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;
	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}

pub async fn build_mint_capability(
	state: &AppState,
	capability_hash: &[u8; 32],
) -> Result<(Value, String), TxBuildError> {
	let (type_code_hash, dep_tx_hash) = cap_nft_type_env()?;

	let agent_lock_args = super::job::parse_lock_args_20(&state.lock_args)?;

	let proof_data = create_attestation(&state.private_key, &agent_lock_args, capability_hash)?;
	let nft_data = encode_capability_data(&agent_lock_args, capability_hash, &proof_data);

	let occupied_bytes = 8 + 53 + 33 + nft_data.len() as u64;
	let nft_capacity = std::cmp::max(occupied_bytes * 100_000_000, CAP_NFT_CELL_MIN);

	let needed = nft_capacity + ESTIMATED_FEE + MIN_CELL_CAPACITY;
	let agent_lock = our_lock(state);
	let cells = state.ckb.get_cells_by_lock(&agent_lock, 200).await?;

	let mut inputs = Vec::new();
	let mut input_capacity: u64 = 0;
	for cell in &cells.objects {
		if cell.output.type_script.is_some() {
			continue;
		}
		let cap = parse_capacity_hex(&cell.output.capacity)?;
		inputs.push(json!({ "previous_output": cell.out_point, "since": "0x0" }));
		input_capacity += cap;
		if input_capacity >= needed {
			break;
		}
	}
	if input_capacity < needed {
		return Err(TxBuildError::InsufficientFunds {
			need: needed as f64 / 1e8,
			have: input_capacity as f64 / 1e8,
		});
	}

	let change_capacity = input_capacity - nft_capacity - ESTIMATED_FEE;
	let witnesses = placeholder_witnesses(inputs.len());

	let tx = json!({
		"version": "0x0",
		"cell_deps": [
			{ "out_point": { "tx_hash": SECP256K1_DEP_TX_HASH, "index": "0x0" }, "dep_type": "dep_group" },
			{ "out_point": { "tx_hash": dep_tx_hash, "index": "0x0" }, "dep_type": "code" },
		],
		"header_deps": [],
		"inputs": inputs,
		"outputs": [
			{
				"capacity": format!("{:#x}", nft_capacity),
				"lock": our_lock(state),
				"type": { "code_hash": type_code_hash, "hash_type": "data1", "args": "0x" },
			},
			{
				"capacity": format!("{:#x}", change_capacity),
				"lock": our_lock(state),
				"type": null,
			},
		],
		"outputs_data": [format!("0x{}", hex::encode(&nft_data)), "0x"],
		"witnesses": witnesses,
	});

	let tx_hash = compute_raw_tx_hash(&tx)?;
	let signature = sign_tx(&tx_hash, &state.private_key, inputs.len())?;
	let mut tx = tx;
	inject_witness(&mut tx, &signature);

	Ok((tx, tx_hash))
}
