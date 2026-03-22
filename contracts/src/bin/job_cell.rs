#![no_std]
#![no_main]
#![allow(unexpected_cfgs)]

use ckb_hash::new_blake2b;
use ckb_std::{
	ckb_constants::Source,
	ckb_types::prelude::Entity,
	default_alloc,
	dummy_atomic,
	entry,
	error::SysError,
	high_level::{
		load_cell_capacity, load_cell_data, load_cell_lock, load_cell_type_hash, load_header,
		load_witness_args,
	},
};

default_alloc!();
entry!(program_entry);

const ERR_SYS: i8 = 1;
const ERR_INVALID_DATA: i8 = 2;
const ERR_INVALID_STATUS: i8 = 3;
const ERR_IMMUTABLE_FIELD_CHANGED: i8 = 4;
const ERR_ZERO_REWARD: i8 = 5;
const ERR_ZERO_POSTER: i8 = 6;
const ERR_ZERO_WORKER: i8 = 7;
const ERR_JOB_EXPIRED: i8 = 8;
const ERR_NOT_EXPIRED: i8 = 9;
const ERR_CAPABILITY_NOT_FOUND: i8 = 10;
const ERR_WORKER_UNDERPAID: i8 = 11;
const ERR_MISSING_RESULT: i8 = 12;
const ERR_INVALID_RESULT_HASH: i8 = 13;

const DATA_MIN: usize = 122;

const STATUS_OPEN: u8 = 0;
const STATUS_RESERVED: u8 = 1;
const STATUS_CLAIMED: u8 = 2;
const STATUS_COMPLETED: u8 = 3;
const STATUS_EXPIRED: u8 = 4;

fn sys_err(_: SysError) -> i8 {
	ERR_SYS
}

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
		validate_transition()
	}
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;
	if data.len() < DATA_MIN {
		return Err(ERR_INVALID_DATA);
	}
	if data[0] != 0 {
		return Err(ERR_INVALID_DATA);
	}
	if data[1] != STATUS_OPEN {
		return Err(ERR_INVALID_STATUS);
	}

	if data[2..22].iter().all(|&b| b == 0) {
		return Err(ERR_ZERO_POSTER);
	}

	let reward = read_u64_le(&data[42..50]).ok_or(ERR_INVALID_DATA)?;
	if reward == 0 {
		return Err(ERR_ZERO_REWARD);
	}

	Ok(())
}

fn validate_transition() -> Result<(), i8> {
	let old = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;
	if old.len() < DATA_MIN {
		return Err(ERR_INVALID_DATA);
	}

	let new = match load_cell_data(0, Source::GroupOutput) {
		Ok(d) => d,
		Err(SysError::IndexOutOfBound) => return validate_destruction(&old),
		Err(e) => return Err(sys_err(e)),
	};

	if new.len() < DATA_MIN {
		return Err(ERR_INVALID_DATA);
	}

	let old_status = old[1];
	let new_status = new[1];

	let valid_transition = match (old_status, new_status) {
		(STATUS_OPEN, STATUS_RESERVED) => true,
		(STATUS_RESERVED, STATUS_CLAIMED) => true,
		(STATUS_CLAIMED, STATUS_COMPLETED) => true,
		(STATUS_OPEN, STATUS_EXPIRED)
		| (STATUS_RESERVED, STATUS_EXPIRED)
		| (STATUS_CLAIMED, STATUS_EXPIRED) => true,
		_ => false,
	};
	if !valid_transition {
		return Err(ERR_INVALID_STATUS);
	}

	let ttl = read_u64_le(&old[50..58]).ok_or(ERR_INVALID_DATA)?;

	if old_status == STATUS_OPEN && new_status == STATUS_RESERVED {
		if ttl > 0 {
			let current_block = load_header_dep_block_number()?;
			if current_block >= ttl {
				return Err(ERR_JOB_EXPIRED);
			}
		}

		if new[22..42].iter().all(|&b| b == 0) {
			return Err(ERR_ZERO_WORKER);
		}

		let cap_hash = &new[58..90];
		if !cap_hash.iter().all(|&b| b == 0) {
			if !verify_capability_in_cell_deps(cap_hash, &new[22..42])? {
				return Err(ERR_CAPABILITY_NOT_FOUND);
			}
		}
	}

	if new_status == STATUS_EXPIRED && old_status != STATUS_OPEN {
		if ttl > 0 {
			let current_block = load_header_dep_block_number()?;
			if current_block < ttl {
				return Err(ERR_NOT_EXPIRED);
			}
		}
	}

	if old[2..22] != new[2..22] {
		return Err(ERR_IMMUTABLE_FIELD_CHANGED);
	}
	if !(old_status == STATUS_OPEN && new_status == STATUS_RESERVED) && old[22..42] != new[22..42] {
		return Err(ERR_IMMUTABLE_FIELD_CHANGED);
	}
	if old[42..50] != new[42..50] {
		return Err(ERR_IMMUTABLE_FIELD_CHANGED);
	}
	if old[50..58] != new[50..58] {
		return Err(ERR_IMMUTABLE_FIELD_CHANGED);
	}
	if old[58..90] != new[58..90] {
		return Err(ERR_IMMUTABLE_FIELD_CHANGED);
	}
	if old[90..] != new[90..] {
		return Err(ERR_IMMUTABLE_FIELD_CHANGED);
	}

	if !(old_status == STATUS_OPEN && new_status == STATUS_RESERVED)
		&& old[22..42] != new[22..42]
	{
		return Err(ERR_IMMUTABLE_FIELD_CHANGED);
	}

	Ok(())
}

fn validate_destruction(old: &[u8]) -> Result<(), i8> {
	let status = old[1];
	match status {
		STATUS_OPEN => Ok(()),

		STATUS_RESERVED => {
			let ttl = read_u64_le(&old[50..58]).ok_or(ERR_INVALID_DATA)?;
			if ttl > 0 {
				let current_block = load_header_dep_block_number()?;
				if current_block < ttl {
					return Err(ERR_NOT_EXPIRED);
				}
			}
			Ok(())
		}

		STATUS_CLAIMED => {
			verify_result_binding(old)?;
			verify_settlement_outputs(old)
		}

		STATUS_COMPLETED | STATUS_EXPIRED => Ok(()),

		_ => Err(ERR_INVALID_STATUS),
	}
}

fn verify_result_binding(old: &[u8]) -> Result<(), i8> {
	let desc_hash = &old[90..122];
	if desc_hash.iter().all(|&b| b == 0) {
		return Ok(());
	}

	let witness = load_witness_args(0, Source::GroupInput).map_err(sys_err)?;
	let input_type = witness.input_type().to_opt().ok_or(ERR_MISSING_RESULT)?;
	let proof = input_type.raw_data();
	if proof.len() < 32 {
		return Err(ERR_MISSING_RESULT);
	}

	let result_hash = &proof[..32];
	let result_data = &proof[32..];

	let mut hasher = new_blake2b();
	hasher.update(desc_hash);
	hasher.update(result_data);
	let mut computed = [0u8; 32];
	hasher.finalize(&mut computed);

	if computed != *result_hash {
		return Err(ERR_INVALID_RESULT_HASH);
	}

	Ok(())
}

fn verify_settlement_outputs(old: &[u8]) -> Result<(), i8> {
	let worker_lock_args = &old[22..42];
	let reward = read_u64_le(&old[42..50]).ok_or(ERR_INVALID_DATA)?;

	let mut worker_total: u64 = 0;
	let mut i = 0;
	loop {
		match load_cell_lock(i, Source::Output) {
			Ok(lock) => {
				if lock.args().raw_data().as_ref() == worker_lock_args {
					let cap = load_cell_capacity(i, Source::Output).map_err(sys_err)?;
					worker_total = worker_total.checked_add(cap).ok_or(ERR_INVALID_DATA)?;
				}
				i += 1;
			}
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		}
	}

	if worker_total < reward {
		return Err(ERR_WORKER_UNDERPAID);
	}

	Ok(())
}

fn read_u64_le(bytes: &[u8]) -> Option<u64> {
	let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
	Some(u64::from_le_bytes(arr))
}

fn load_header_dep_block_number() -> Result<u64, i8> {
	let header = load_header(0, Source::HeaderDep).map_err(sys_err)?;
	read_u64_le(header.raw().number().as_slice()).ok_or(ERR_INVALID_DATA)
}

fn verify_capability_in_cell_deps(cap_hash: &[u8], worker_args: &[u8]) -> Result<bool, i8> {
	let mut i = 0;
	loop {
		let data = match load_cell_data(i, Source::CellDep) {
			Ok(d) => d,
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		};

		if data.len() >= 54 && data[22..54] == *cap_hash && data[2..22] == *worker_args {
			let has_type = load_cell_type_hash(i, Source::CellDep)
				.map_err(sys_err)?
				.is_some();
			if has_type {
				return Ok(true);
			}
		}

		i += 1;
	}
	Ok(false)
}
