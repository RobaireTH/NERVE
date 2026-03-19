// Capability NFT Type Script
//
// Represents a verifiable capability claim for an agent.
// Two proof modes: attestation (signed) and reputation-chain-backed.
//
// Invariants:
//   Creation:
//     - version = 0, proof_type must be 0 (attestation) or 1 (reputation-backed).
//     - agent_lock_args must be non-zero (ties NFT to an agent identity).
//     - capability_hash must be non-zero.
//     - proof_data (bytes 54+) must be non-empty.
//     - For proof_type=1: proof_root_snapshot must match a live reputation
//       cell for this agent in cell_deps (on-chain cross-referencing).
//   Spending:
//     - The NFT cell must reappear in outputs (capability cannot be destroyed unilaterally).
//     - capability_hash and agent_lock_args are immutable.
//
// Cell data layout (54+ bytes, little-endian):
//   [0]       version: u8         = 0
//   [1]       proof_type: u8      0=attestation (ZK reserved for later)
//   [2..22]   agent_lock_args:    [u8; 20]
//   [22..54]  capability_hash:    [u8; 32]
//   [54..]    proof_data:         bytes (attestation or ZK proof)

#![no_std]
#![no_main]
#![allow(unexpected_cfgs)]

use ckb_std::{
	ckb_constants::Source,
	default_alloc,
	entry,
	error::SysError,
	high_level::{load_cell_data, load_cell_type_hash},
};

default_alloc!();
entry!(program_entry);

const ERR_SYS: i8 = 1;
const ERR_INVALID_DATA: i8 = 2;
const ERR_INVALID_PROOF_TYPE: i8 = 3;
const ERR_ZERO_AGENT: i8 = 4;
const ERR_ZERO_CAP_HASH: i8 = 5;
const ERR_EMPTY_PROOF: i8 = 6;
const ERR_IMMUTABLE_FIELD_CHANGED: i8 = 7;
const ERR_NFT_DESTROYED: i8 = 8;
const ERR_PROOF_ROOT_MISMATCH: i8 = 9;

const DATA_MIN: usize = 54;

fn sys_err(_: SysError) -> i8 { ERR_SYS }

fn program_entry() -> i8 {
	match run() {
		Ok(()) => 0,
		Err(code) => code,
	}
}

fn run() -> Result<(), i8> {
	let creation_mode = match load_cell_data(0, Source::GroupInput) {
		Err(SysError::IndexOutOfBound) => true,
		Ok(_) => false,
		Err(e) => return Err(sys_err(e)),
	};

	if creation_mode {
		validate_creation()
	} else {
		validate_transfer()
	}
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;
	if data.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }
	if data[0] != 0 { return Err(ERR_INVALID_DATA); }

	let proof_type = data[1];
	if proof_type > 1 { return Err(ERR_INVALID_PROOF_TYPE); }

	if data[2..22].iter().all(|&b| b == 0) { return Err(ERR_ZERO_AGENT); }
	if data[22..54].iter().all(|&b| b == 0) { return Err(ERR_ZERO_CAP_HASH); }
	if data.len() <= DATA_MIN { return Err(ERR_EMPTY_PROOF); }

	// proof_type=1 (reputation-chain-backed): proof_data must be exactly 64 bytes
	// (proof_root_snapshot[32] + settlement_hash[32]), both non-zero.
	// The proof_root_snapshot must match a live reputation cell for this agent in cell_deps.
	if proof_type == 1 {
		let proof_data = &data[DATA_MIN..];
		if proof_data.len() != 64 { return Err(ERR_EMPTY_PROOF); }
		if proof_data[..32].iter().all(|&b| b == 0) { return Err(ERR_EMPTY_PROOF); }
		if proof_data[32..].iter().all(|&b| b == 0) { return Err(ERR_EMPTY_PROOF); }

		// Cross-reference: verify the proof_root_snapshot matches the agent's reputation cell.
		let agent_lock_args = &data[2..22];
		let proof_root_snapshot = &proof_data[..32];
		if !verify_proof_root_in_cell_deps(agent_lock_args, proof_root_snapshot)? {
			return Err(ERR_PROOF_ROOT_MISMATCH);
		}
	}

	Ok(())
}

/// Checks cell_deps for a reputation cell whose agent_lock_args matches and
/// whose proof_root at [46..78] matches `expected_proof_root`.
///
/// Reputation data layout: [0] version=0, ..., [26..46] agent_lock_args, [46..78] proof_root.
/// The cell must have a type script to prevent spoofing.
fn verify_proof_root_in_cell_deps(
	agent_lock_args: &[u8],
	expected_proof_root: &[u8],
) -> Result<bool, i8> {
	let mut i = 0;
	loop {
		let data = match load_cell_data(i, Source::CellDep) {
			Ok(d) => d,
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		};

		if data.len() >= 110 && data[0] == 0 {
			if data[26..46] == *agent_lock_args {
				let has_type = load_cell_type_hash(i, Source::CellDep)
					.map_err(sys_err)?
					.is_some();
				if has_type {
					if data[46..78] == *expected_proof_root {
						return Ok(true);
					}
				}
			}
		}

		i += 1;
	}
	Ok(false)
}

fn validate_transfer() -> Result<(), i8> {
	let old = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;

	// The NFT must reappear in outputs; it cannot be silently destroyed.
	let new = match load_cell_data(0, Source::GroupOutput) {
		Ok(d) => d,
		Err(SysError::IndexOutOfBound) => return Err(ERR_NFT_DESTROYED),
		Err(e) => return Err(sys_err(e)),
	};

	if old.len() < DATA_MIN || new.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }

	// Immutable: agent_lock_args and capability_hash.
	if old[2..22] != new[2..22] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); }
	if old[22..54] != new[22..54] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); }

	Ok(())
}
