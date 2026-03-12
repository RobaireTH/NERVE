// Mock AMM Type Script
//
// Minimal constant-product AMM for demo purposes. Validates swap cells so the
// NERVE DeFi worker can execute a real on-chain swap transaction.
//
// Design: a single "pool" cell guarded by this type script holds two u128
// reserves (CKB and TEST_TOKEN). A swap consumes the pool cell and recreates
// it with updated reserves that satisfy x * y >= k (the constant-product
// invariant, allowing rounding in the pool's favour).
//
// Cell data layout (33 bytes minimum, little-endian):
//   [0]       version: u8          = 0
//   [1..17]   reserve_ckb: u128 LE (in shannons)
//   [17..33]  reserve_token: u128 LE
//
// Rules:
//   Creation — version must be 0, both reserves must be > 0.
//   Update   — new_reserve_ckb * new_reserve_token >= old_reserve_ckb * old_reserve_token.
//              This allows any swap direction as long as the pool doesn't lose value.
//   Destruction — always allowed (pool owner can withdraw).

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
const ERR_ZERO_RESERVE: i8 = 3;
const ERR_K_DECREASED: i8 = 4;

const DATA_MIN: usize = 33;

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
		validate_swap()
	}
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;
	if data.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }
	if data[0] != 0 { return Err(ERR_INVALID_DATA); }

	let reserve_ckb = read_u128_le(&data[1..17]).ok_or(ERR_INVALID_DATA)?;
	let reserve_token = read_u128_le(&data[17..33]).ok_or(ERR_INVALID_DATA)?;

	if reserve_ckb == 0 || reserve_token == 0 {
		return Err(ERR_ZERO_RESERVE);
	}

	Ok(())
}

fn validate_swap() -> Result<(), i8> {
	let old = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;
	if old.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }

	// Destruction (pool withdrawal) — always allowed.
	let new = match load_cell_data(0, Source::GroupOutput) {
		Ok(d) => d,
		Err(SysError::IndexOutOfBound) => return Ok(()),
		Err(e) => return Err(sys_err(e)),
	};
	if new.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }

	let old_ckb = read_u128_le(&old[1..17]).ok_or(ERR_INVALID_DATA)?;
	let old_token = read_u128_le(&old[17..33]).ok_or(ERR_INVALID_DATA)?;
	let new_ckb = read_u128_le(&new[1..17]).ok_or(ERR_INVALID_DATA)?;
	let new_token = read_u128_le(&new[17..33]).ok_or(ERR_INVALID_DATA)?;

	if new_ckb == 0 || new_token == 0 {
		return Err(ERR_ZERO_RESERVE);
	}

	// Constant-product check: new_x * new_y >= old_x * old_y.
	// Use u128 multiplication — reserves are u128 so the product can overflow.
	// For demo purposes we check with saturating multiplication which is safe
	// as long as reserves stay under ~2^64 (they will for testnet demo amounts).
	let old_k = old_ckb.saturating_mul(old_token);
	let new_k = new_ckb.saturating_mul(new_token);

	if new_k < old_k {
		return Err(ERR_K_DECREASED);
	}

	Ok(())
}

fn read_u128_le(bytes: &[u8]) -> Option<u128> {
	let arr: [u8; 16] = bytes.get(..16)?.try_into().ok()?;
	Some(u128::from_le_bytes(arr))
}
