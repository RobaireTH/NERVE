// Agent Identity Type Script
//
// This type script enforces two invariants at CKB consensus level:
//
// CREATION MODE (no GroupInput cells with this type):
//   - Cell data must be at least 50 bytes with a valid layout.
//   - spending_limit_per_tx must be > 0.
//   - daily_limit must be >= spending_limit_per_tx.
//
// SPENDING MODE (GroupInput cells with this type exist):
//   - Total capacity flowing to non-agent addresses must not exceed
//     spending_limit_per_tx encoded in the input identity cell.
//
// Cell data layout (little-endian):
//   [0]      version: u8       = 0
//   [1..34]  pubkey: [u8; 33]  compressed secp256k1 pubkey
//   [34..42] spending_limit_per_tx: u64  (shannons)
//   [42..50] daily_limit: u64           (shannons)

#![no_std]
#![no_main]
// ckb-std v0.16 emits a `native-simulator` cfg check inside entry! that Rust doesn't know about.
#![allow(unexpected_cfgs)]

use ckb_std::{
	ckb_constants::Source,
	default_alloc,
	entry,
	error::SysError,
	high_level::{load_cell_capacity, load_cell_data, load_cell_lock_hash},
};

default_alloc!();
entry!(program_entry);

// Error codes returned to CKB-VM. 0 = success; non-zero = failure.
const ERR_SYS: i8 = 1;
const ERR_INVALID_DATA: i8 = 2;
const ERR_INVALID_SPENDING_LIMIT: i8 = 3;
const ERR_INVALID_DAILY_LIMIT: i8 = 4;
const ERR_SPENDING_LIMIT_EXCEEDED: i8 = 5;

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
	// Detect mode: if no GroupInput cells with this type exist, we're creating.
	let creation_mode = match load_cell_data(0, Source::GroupInput) {
		Err(SysError::IndexOutOfBound) => true,
		Ok(_) => false,
		Err(e) => return Err(sys_err(e)),
	};

	if creation_mode {
		validate_creation()
	} else {
		validate_spending()
	}
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;

	// Minimum: version(1) + pubkey(33) + spending_limit(8) + daily_limit(8) = 50 bytes.
	if data.len() < 50 {
		return Err(ERR_INVALID_DATA);
	}

	// Version must be 0.
	if data[0] != 0 {
		return Err(ERR_INVALID_DATA);
	}

	let spending_limit = read_u64_le(&data[34..42]).ok_or(ERR_INVALID_DATA)?;
	if spending_limit == 0 {
		return Err(ERR_INVALID_SPENDING_LIMIT);
	}

	let daily_limit = read_u64_le(&data[42..50]).ok_or(ERR_INVALID_DATA)?;
	if daily_limit < spending_limit {
		return Err(ERR_INVALID_DAILY_LIMIT);
	}

	Ok(())
}

fn validate_spending() -> Result<(), i8> {
	// Read the spending limit from the agent identity cell being consumed.
	let identity_data = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;
	if identity_data.len() < 50 {
		return Err(ERR_INVALID_DATA);
	}

	let spending_limit = read_u64_le(&identity_data[34..42]).ok_or(ERR_INVALID_DATA)?;

	// Get the agent's own lock hash so we can exclude self-transfers.
	let agent_lock_hash = load_cell_lock_hash(0, Source::GroupInput).map_err(sys_err)?;

	// Sum capacity flowing to addresses other than the agent.
	let mut transferred_to_others: u64 = 0;
	let mut idx = 0;
	loop {
		match load_cell_lock_hash(idx, Source::Output) {
			Ok(lock_hash) => {
				if lock_hash != agent_lock_hash {
					let cap = load_cell_capacity(idx, Source::Output).map_err(sys_err)?;
					transferred_to_others = transferred_to_others.saturating_add(cap);
				}
				idx += 1;
			}
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		}
	}

	if transferred_to_others > spending_limit {
		return Err(ERR_SPENDING_LIMIT_EXCEEDED);
	}

	Ok(())
}

fn read_u64_le(bytes: &[u8]) -> Option<u64> {
	let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
	Some(u64::from_le_bytes(arr))
}
