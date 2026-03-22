#![no_std]
#![no_main]
#![allow(unexpected_cfgs)]

use ckb_std::{
	ckb_constants::Source,
	default_alloc,
	dummy_atomic,
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

	if proof_type == 1 {
		let proof_data = &data[DATA_MIN..];
		if proof_data.len() != 64 { return Err(ERR_EMPTY_PROOF); }
		if proof_data[..32].iter().all(|&b| b == 0) { return Err(ERR_EMPTY_PROOF); }
		if proof_data[32..].iter().all(|&b| b == 0) { return Err(ERR_EMPTY_PROOF); }

		let agent_lock_args = &data[2..22];
		let proof_root_snapshot = &proof_data[..32];
		if !verify_proof_root_in_cell_deps(agent_lock_args, proof_root_snapshot)? {
			return Err(ERR_PROOF_ROOT_MISMATCH);
		}
	}

	Ok(())
}

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

	let new = match load_cell_data(0, Source::GroupOutput) {
		Ok(d) => d,
		Err(SysError::IndexOutOfBound) => return Err(ERR_NFT_DESTROYED),
		Err(e) => return Err(sys_err(e)),
	};

	if old.len() < DATA_MIN || new.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }

	if old[2..22] != new[2..22] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); }
	if old[22..54] != new[22..54] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); }

	Ok(())
}
