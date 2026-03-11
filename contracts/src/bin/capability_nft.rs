// Capability NFT Type Script
//
// Represents a verifiable capability claim for an agent.
// Attestation mode: off-chain signed proof bytes in data field.
//
// Invariants:
//   Creation:
//     - version = 0, proof_type must be 0 (attestation) for now.
//     - agent_lock_args must be non-zero (ties NFT to an agent identity).
//     - capability_hash must be non-zero.
//     - proof_data (bytes 54+) must be non-empty.
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
	high_level::load_cell_data,
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

	if data[1] != 0 { return Err(ERR_INVALID_PROOF_TYPE); }  // only attestation supported now

	if data[2..22].iter().all(|&b| b == 0) { return Err(ERR_ZERO_AGENT); }
	if data[22..54].iter().all(|&b| b == 0) { return Err(ERR_ZERO_CAP_HASH); }
	if data.len() <= DATA_MIN { return Err(ERR_EMPTY_PROOF); }

	Ok(())
}

fn validate_transfer() -> Result<(), i8> {
	let old = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;

	// The NFT must reappear in outputs — it cannot be silently destroyed.
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
