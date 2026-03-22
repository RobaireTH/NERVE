#![no_std]
#![no_main]
#![allow(unexpected_cfgs)]

use ckb_hash::blake2b_256;
use ckb_std::{
	ckb_constants::Source,
	default_alloc,
	dummy_atomic,
	entry,
	error::SysError,
	high_level::{load_cell_data, load_input_since},
	type_id::check_type_id,
};

default_alloc!();
entry!(program_entry);

const ERR_SYS: i8 = 1;
const ERR_INVALID_DATA: i8 = 2;
const ERR_INVALID_TRANSITION: i8 = 3;
const ERR_IMMUTABLE_FIELD: i8 = 4;
const ERR_WRONG_COUNTER: i8 = 5;
const ERR_ZERO_AGENT: i8 = 6;
const ERR_TYPE_ID: i8 = 7;
const ERR_DISPUTE_WINDOW_ACTIVE: i8 = 8;
const ERR_PENDING_NOT_CLEARED: i8 = 9;
const ERR_INVALID_PROOF_ROOT: i8 = 10;
const ERR_INVALID_SETTLEMENT_HASH: i8 = 11;
const ERR_PROOF_ROOT_MISMATCH: i8 = 12;

const DATA_SIZE: usize = 110;

// Bits 63-61 must be 000 for absolute block number metric.
const SINCE_METRIC_MASK: u64 = 0xE000_0000_0000_0000;

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
	check_type_id(0).map_err(|_| ERR_TYPE_ID)?;

	let creation_mode = match load_cell_data(0, Source::GroupInput) {
		Err(SysError::IndexOutOfBound) => true,
		Ok(_) => false,
		Err(e) => return Err(sys_err(e)),
	};

	if creation_mode {
		validate_creation()
	} else {
		validate_update()
	}
}

fn is_all_zero(slice: &[u8]) -> bool {
	slice.iter().all(|&b| b == 0)
}

fn read_bytes_32(data: &[u8], offset: usize) -> Option<[u8; 32]> {
	let slice = data.get(offset..offset + 32)?;
	let arr: [u8; 32] = slice.try_into().ok()?;
	Some(arr)
}

fn compute_new_proof_root(old_root: &[u8; 32], settlement_hash: &[u8; 32]) -> [u8; 32] {
	let mut preimage = [0u8; 64];
	preimage[..32].copy_from_slice(old_root);
	preimage[32..].copy_from_slice(settlement_hash);
	blake2b_256(preimage)
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;
	if data.len() < DATA_SIZE {
		return Err(ERR_INVALID_DATA);
	}

	if data[0] != 0 {
		return Err(ERR_INVALID_DATA);
	}

	if data[1] != 0 {
		return Err(ERR_INVALID_TRANSITION);
	}

	if read_u64_le(&data[2..10]).ok_or(ERR_INVALID_DATA)? != 0 {
		return Err(ERR_INVALID_DATA);
	}
	if read_u64_le(&data[10..18]).ok_or(ERR_INVALID_DATA)? != 0 {
		return Err(ERR_INVALID_DATA);
	}

	if read_u64_le(&data[18..26]).ok_or(ERR_INVALID_DATA)? != 0 {
		return Err(ERR_INVALID_DATA);
	}

	if is_all_zero(&data[26..46]) {
		return Err(ERR_ZERO_AGENT);
	}

	if !is_all_zero(&data[46..78]) {
		return Err(ERR_INVALID_PROOF_ROOT);
	}
	if !is_all_zero(&data[78..110]) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	Ok(())
}

fn validate_update() -> Result<(), i8> {
	let old = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;
	let new = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;

	if old.len() < DATA_SIZE || new.len() < DATA_SIZE {
		return Err(ERR_INVALID_DATA);
	}

	if old[26..46] != new[26..46] {
		return Err(ERR_IMMUTABLE_FIELD);
	}

	if old[0] != 0 || new[0] != 0 {
		return Err(ERR_INVALID_DATA);
	}

	let old_pending = old[1];
	let new_pending = new[1];
	let old_completed = read_u64_le(&old[2..10]).ok_or(ERR_INVALID_DATA)?;
	let old_abandoned = read_u64_le(&old[10..18]).ok_or(ERR_INVALID_DATA)?;
	let new_completed = read_u64_le(&new[2..10]).ok_or(ERR_INVALID_DATA)?;
	let new_abandoned = read_u64_le(&new[10..18]).ok_or(ERR_INVALID_DATA)?;

	match (old_pending, new_pending) {
		(0, 1) | (0, 2) => {
			if new_completed != old_completed {
				return Err(ERR_WRONG_COUNTER);
			}
			if new_abandoned != old_abandoned {
				return Err(ERR_WRONG_COUNTER);
			}
			let expires_at = read_u64_le(&new[18..26]).ok_or(ERR_INVALID_DATA)?;
			if expires_at == 0 {
				return Err(ERR_INVALID_TRANSITION);
			}
			validate_propose(&old, &new)?;
		}
		(1, 0) => {
			validate_dispute_window_elapsed(&old)?;

			if new_completed != old_completed + 1 {
				return Err(ERR_WRONG_COUNTER);
			}
			if new_abandoned != old_abandoned {
				return Err(ERR_WRONG_COUNTER);
			}
			validate_pending_cleared(&new)?;
			validate_finalize(&old, &new)?;
		}
		(2, 0) => {
			validate_dispute_window_elapsed(&old)?;

			if new_completed != old_completed {
				return Err(ERR_WRONG_COUNTER);
			}
			if new_abandoned != old_abandoned + 1 {
				return Err(ERR_WRONG_COUNTER);
			}
			validate_pending_cleared(&new)?;
			validate_finalize(&old, &new)?;
		}
		_ => return Err(ERR_INVALID_TRANSITION),
	}

	Ok(())
}

fn validate_propose(old: &[u8], new: &[u8]) -> Result<(), i8> {
	let old_proof_root = read_bytes_32(old, 46).ok_or(ERR_INVALID_DATA)?;
	let new_proof_root = read_bytes_32(new, 46).ok_or(ERR_INVALID_DATA)?;

	if old_proof_root != new_proof_root {
		return Err(ERR_PROOF_ROOT_MISMATCH);
	}

	let old_settlement = read_bytes_32(old, 78).ok_or(ERR_INVALID_DATA)?;
	let new_settlement = read_bytes_32(new, 78).ok_or(ERR_INVALID_DATA)?;

	if !is_all_zero(&old_settlement) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	if is_all_zero(&new_settlement) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	Ok(())
}

fn validate_finalize(old: &[u8], new: &[u8]) -> Result<(), i8> {
	let old_proof_root = read_bytes_32(old, 46).ok_or(ERR_INVALID_DATA)?;
	let old_settlement = read_bytes_32(old, 78).ok_or(ERR_INVALID_DATA)?;
	let new_proof_root = read_bytes_32(new, 46).ok_or(ERR_INVALID_DATA)?;
	let new_settlement = read_bytes_32(new, 78).ok_or(ERR_INVALID_DATA)?;

	let expected = compute_new_proof_root(&old_proof_root, &old_settlement);
	if new_proof_root != expected {
		return Err(ERR_PROOF_ROOT_MISMATCH);
	}

	if !is_all_zero(&new_settlement) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	Ok(())
}

fn validate_dispute_window_elapsed(old_data: &[u8]) -> Result<(), i8> {
	let pending_expires_at = read_u64_le(&old_data[18..26]).ok_or(ERR_INVALID_DATA)?;
	if pending_expires_at == 0 {
		return Err(ERR_INVALID_TRANSITION);
	}

	let since = load_input_since(0, Source::GroupInput).map_err(sys_err)?;

	if since & SINCE_METRIC_MASK != 0 {
		return Err(ERR_DISPUTE_WINDOW_ACTIVE);
	}

	if since < pending_expires_at {
		return Err(ERR_DISPUTE_WINDOW_ACTIVE);
	}

	Ok(())
}

fn validate_pending_cleared(new_data: &[u8]) -> Result<(), i8> {
	let new_expires_at = read_u64_le(&new_data[18..26]).ok_or(ERR_INVALID_DATA)?;
	if new_expires_at != 0 {
		return Err(ERR_PENDING_NOT_CLEARED);
	}
	Ok(())
}

fn read_u64_le(bytes: &[u8]) -> Option<u64> {
	let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
	Some(u64::from_le_bytes(arr))
}
