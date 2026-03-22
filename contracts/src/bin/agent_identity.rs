#![no_std]
#![no_main]
#![allow(unexpected_cfgs)]

use ckb_std::{
	ckb_constants::Source,
	ckb_types::prelude::Entity,
	default_alloc,
	dummy_atomic,
	entry,
	error::SysError,
	high_level::{
		load_cell_capacity, load_cell_data, load_cell_lock, load_cell_lock_hash,
		load_cell_type_hash, load_header,
	},
	type_id::check_type_id,
};

default_alloc!();
entry!(program_entry);

const ERR_SYS: i8 = 1;
const ERR_INVALID_DATA: i8 = 2;
const ERR_INVALID_SPENDING_LIMIT: i8 = 3;
const ERR_INVALID_DAILY_LIMIT: i8 = 4;
const ERR_SPENDING_LIMIT_EXCEEDED: i8 = 5;
const ERR_TYPE_ID: i8 = 6;
const ERR_BURN_FORBIDDEN: i8 = 7;
const ERR_INVALID_REVENUE_SHARE: i8 = 8;
const ERR_PARENT_NOT_SIGNED: i8 = 9;
const ERR_SPENDING_EXCEEDS_PARENT: i8 = 10;
const ERR_IDENTITY_DATA_CHANGED: i8 = 11;
const ERR_DAILY_LIMIT_EXCEEDED: i8 = 12;
const ERR_INVALID_ACCUMULATOR: i8 = 13;

// 6 epochs ≈ 24 hours on mainnet (each epoch targets ~4 hours).
const EPOCHS_PER_DAY: u64 = 6;

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

	let has_input = match load_cell_data(0, Source::GroupInput) {
		Err(SysError::IndexOutOfBound) => false,
		Ok(_) => true,
		Err(e) => return Err(sys_err(e)),
	};

	let has_output = match load_cell_data(0, Source::GroupOutput) {
		Err(SysError::IndexOutOfBound) => false,
		Ok(_) => true,
		Err(e) => return Err(sys_err(e)),
	};

	match (has_input, has_output) {
		(false, true) => validate_creation(),
		(true, true) => validate_spending(),
		(true, false) => Err(ERR_BURN_FORBIDDEN),
		(false, false) => Err(ERR_SYS),
	}
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;

	if data.len() < 88 {
		return Err(ERR_INVALID_DATA);
	}
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

	let revenue_share_bps = read_u16_le(&data[70..72]).ok_or(ERR_INVALID_DATA)?;
	if revenue_share_bps > 10000 {
		return Err(ERR_INVALID_REVENUE_SHARE);
	}

	let daily_spent = read_u64_le(&data[72..80]).ok_or(ERR_INVALID_DATA)?;
	if daily_spent != 0 {
		return Err(ERR_INVALID_ACCUMULATOR);
	}

	let last_reset_epoch = read_u64_le(&data[80..88]).ok_or(ERR_INVALID_DATA)?;
	if last_reset_epoch != 0 {
		return Err(ERR_INVALID_ACCUMULATOR);
	}

	let parent_lock_args = &data[50..70];
	let has_parent = !parent_lock_args.iter().all(|&b| b == 0);

	if has_parent {
		verify_parent_signed_and_within_limits(parent_lock_args, spending_limit)?;
	}

	Ok(())
}

fn validate_spending() -> Result<(), i8> {
	let identity_data = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;
	if identity_data.len() < 88 {
		return Err(ERR_INVALID_DATA);
	}

	let output_data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;
	if output_data.len() < 88 {
		return Err(ERR_INVALID_DATA);
	}

	if identity_data[..72] != output_data[..72] {
		return Err(ERR_IDENTITY_DATA_CHANGED);
	}

	let spending_limit = read_u64_le(&identity_data[34..42]).ok_or(ERR_INVALID_DATA)?;

	let agent_lock_hash = load_cell_lock_hash(0, Source::GroupInput).map_err(sys_err)?;

	let mut agent_input_total: u64 = 0;
	let mut idx = 0;
	loop {
		match load_cell_lock_hash(idx, Source::Input) {
			Ok(lock_hash) => {
				if lock_hash == agent_lock_hash {
					let cap = load_cell_capacity(idx, Source::Input).map_err(sys_err)?;
					agent_input_total =
						agent_input_total.checked_add(cap).ok_or(ERR_INVALID_DATA)?;
				}
				idx += 1;
			}
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		}
	}

	let mut agent_output_total: u64 = 0;
	idx = 0;
	loop {
		match load_cell_lock_hash(idx, Source::Output) {
			Ok(lock_hash) => {
				if lock_hash == agent_lock_hash {
					let cap = load_cell_capacity(idx, Source::Output).map_err(sys_err)?;
					agent_output_total =
						agent_output_total.checked_add(cap).ok_or(ERR_INVALID_DATA)?;
				}
				idx += 1;
			}
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		}
	}

	let transferred_to_others = agent_input_total.saturating_sub(agent_output_total);

	if transferred_to_others > spending_limit {
		return Err(ERR_SPENDING_LIMIT_EXCEEDED);
	}

	validate_daily_accumulator(&identity_data, &output_data, transferred_to_others)?;

	Ok(())
}

fn validate_daily_accumulator(
	input_data: &[u8],
	output_data: &[u8],
	transferred: u64,
) -> Result<(), i8> {
	let daily_limit = read_u64_le(&input_data[42..50]).ok_or(ERR_INVALID_DATA)?;
	let old_daily_spent = read_u64_le(&input_data[72..80]).ok_or(ERR_INVALID_DATA)?;
	let old_reset_epoch = read_u64_le(&input_data[80..88]).ok_or(ERR_INVALID_DATA)?;

	let current_epoch = load_header_dep_epoch_number()?;
	let old_day = old_reset_epoch / EPOCHS_PER_DAY;
	let current_day = current_epoch / EPOCHS_PER_DAY;

	let base_spent = if current_day != old_day { 0 } else { old_daily_spent };
	let new_daily_spent = base_spent.checked_add(transferred).ok_or(ERR_INVALID_DATA)?;

	if new_daily_spent > daily_limit {
		return Err(ERR_DAILY_LIMIT_EXCEEDED);
	}

	let out_daily_spent = read_u64_le(&output_data[72..80]).ok_or(ERR_INVALID_DATA)?;
	let out_reset_epoch = read_u64_le(&output_data[80..88]).ok_or(ERR_INVALID_DATA)?;

	if out_daily_spent != new_daily_spent {
		return Err(ERR_INVALID_ACCUMULATOR);
	}
	if out_reset_epoch != current_epoch {
		return Err(ERR_INVALID_ACCUMULATOR);
	}

	Ok(())
}

fn load_header_dep_epoch_number() -> Result<u64, i8> {
	let header = load_header(0, Source::HeaderDep).map_err(sys_err)?;
	let epoch_raw = read_u64_le(header.raw().epoch().as_slice()).ok_or(ERR_INVALID_DATA)?;
	Ok(epoch_raw & 0x00FF_FFFF) // lower 24 bits = epoch number
}

fn read_u64_le(bytes: &[u8]) -> Option<u64> {
	let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
	Some(u64::from_le_bytes(arr))
}

fn read_u16_le(bytes: &[u8]) -> Option<u16> {
	let arr: [u8; 2] = bytes.get(..2)?.try_into().ok()?;
	Some(u16::from_le_bytes(arr))
}

fn verify_parent_signed_and_within_limits(
	parent_lock_args: &[u8],
	child_spending_limit: u64,
) -> Result<(), i8> {
	let mut parent_lock_hash: Option<[u8; 32]> = None;
	let mut parent_limit: Option<u64> = None;

	let mut i = 0;
	loop {
		let data = match load_cell_data(i, Source::CellDep) {
			Ok(d) => d,
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		};

		if data.len() >= 88 && data[0] == 0 {
			let has_type = load_cell_type_hash(i, Source::CellDep)
				.map_err(sys_err)?
				.is_some();
			if !has_type {
				i += 1;
				continue;
			}

			let lock = load_cell_lock(i, Source::CellDep).map_err(sys_err)?;
			if lock.args().raw_data().as_ref() == parent_lock_args {
				parent_lock_hash =
					Some(load_cell_lock_hash(i, Source::CellDep).map_err(sys_err)?);
				parent_limit =
					Some(read_u64_le(&data[34..42]).ok_or(ERR_INVALID_DATA)?);
				break;
			}
		}

		i += 1;
	}

	let parent_lh = parent_lock_hash.ok_or(ERR_SPENDING_EXCEEDS_PARENT)?;
	let p_limit = parent_limit.ok_or(ERR_SPENDING_EXCEEDS_PARENT)?;

	if child_spending_limit > p_limit {
		return Err(ERR_SPENDING_EXCEEDS_PARENT);
	}

	let mut j = 0;
	loop {
		match load_cell_lock_hash(j, Source::Input) {
			Ok(lh) => {
				if lh == parent_lh {
					return Ok(());
				}
				j += 1;
			}
			Err(SysError::IndexOutOfBound) => break,
			Err(e) => return Err(sys_err(e)),
		}
	}

	Err(ERR_PARENT_NOT_SIGNED)
}
