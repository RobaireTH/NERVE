// Agent Identity Type Script
//
// Enforces agent identity uniqueness, per-transaction spending caps, daily
// spending accumulation, and parent-child delegation at CKB consensus level.
//
// Type script args layout:
//   [0..32]  type_id: [u8; 32]  (guarantees singleton via CKB Type ID pattern)
//
// Cell data layout (88 bytes, little-endian):
//   [0]      version: u8       = 0
//   [1..34]  pubkey: [u8; 33]  compressed secp256k1 pubkey
//   [34..42] spending_limit_per_tx: u64  (shannons)
//   [42..50] daily_limit: u64           (shannons; enforced on-chain via accumulator)
//   [50..70] parent_lock_args: [u8; 20] (all zeros = root agent)
//   [70..72] revenue_share_bps: u16 LE  (basis points: 1000 = 10%)
//   [72..80] daily_spent: u64           (accumulated spending in current day window)
//   [80..88] last_reset_epoch: u64      (epoch number when accumulator was last reset)
//
// CREATION MODE (no GroupInput):
//   - Type ID must be valid (singleton guarantee).
//   - Data must be exactly 88 bytes, version = 0, spending_limit > 0, daily_limit >= spending_limit.
//   - revenue_share_bps must be <= 10000.
//   - daily_spent and last_reset_epoch must be 0 (fresh accumulator).
//   - If parent_lock_args is non-zero (sub-agent):
//     - Parent must have signed (an input has matching lock.args).
//     - Child spending_limit must not exceed parent's (parent identity in cell_deps).
//
// UPDATE MODE (GroupInput and GroupOutput both present):
//   - Type ID singleton enforced.
//   - Config portion [0..72] must be identical (immutable); accumulator
//     [72..88] is mutable with epoch-based daily limit enforcement:
//     - Reads current epoch from header_deps[0].
//     - Day window = epoch_number / EPOCHS_PER_DAY (6 epochs ≈ 24h on mainnet).
//     - Accumulator resets when the day window changes.
//     - new_daily_spent = (reset? 0 : old_daily_spent) + transferred_to_others.
//     - new_daily_spent must not exceed daily_limit.
//     - Output must contain correct daily_spent and last_reset_epoch.
//   - Total capacity flowing to non-agent addresses must not exceed spending_limit.
//
// BURN PROTECTION:
//   - Destroying the identity cell is forbidden. The cell must always reappear
//     in outputs so the agent cannot escape spending limits.

#![no_std]
#![no_main]
#![allow(unexpected_cfgs)]

use ckb_std::{
	ckb_constants::Source,
	ckb_types::prelude::Entity,
	default_alloc,
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

/// CKB epochs per "day" for the daily spending accumulator.
/// On mainnet, each epoch targets ~4 hours, so 6 epochs ≈ 24 hours.
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
	// Enforce Type ID singleton: at most one input and one output with this type,
	// and on creation the type_id in args must match blake2b(first_input || output_index).
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
		// Creation: new identity cell.
		(false, true) => validate_creation(),
		// Update: spending with preserved identity.
		(true, true) => validate_spending(),
		// Burn attempt: identity cell destroyed — forbidden.
		(true, false) => Err(ERR_BURN_FORBIDDEN),
		// Impossible: no input and no output but type script ran.
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

/// Uses epoch-based day windows: day_number = epoch_number / EPOCHS_PER_DAY.
/// When the day window changes, the accumulator resets. The output cell must
/// contain the correct new daily_spent and the current epoch number.
fn validate_daily_accumulator(
	input_data: &[u8],
	output_data: &[u8],
	transferred: u64,
) -> Result<(), i8> {
	let daily_limit = read_u64_le(&input_data[42..50]).ok_or(ERR_INVALID_DATA)?;
	let old_daily_spent = read_u64_le(&input_data[72..80]).ok_or(ERR_INVALID_DATA)?;
	let old_reset_epoch = read_u64_le(&input_data[80..88]).ok_or(ERR_INVALID_DATA)?;

	// Get current epoch from header_deps[0].
	let current_epoch = load_header_dep_epoch_number()?;
	let old_day = old_reset_epoch / EPOCHS_PER_DAY;
	let current_day = current_epoch / EPOCHS_PER_DAY;

	// Reset accumulator if the day window has changed.
	let base_spent = if current_day != old_day { 0 } else { old_daily_spent };
	let new_daily_spent = base_spent.checked_add(transferred).ok_or(ERR_INVALID_DATA)?;

	if new_daily_spent > daily_limit {
		return Err(ERR_DAILY_LIMIT_EXCEEDED);
	}

	// Verify the output cell contains the correct accumulator values.
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

/// Reads the epoch number from header_deps[0].
///
/// CKB epoch encoding: lower 24 bits = epoch number.
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

/// Finds the parent identity cell in cell_deps, verifies the child's spending_limit
/// does not exceed the parent's, and confirms a TX input carries the parent's full
/// lock_hash (not just lock.args) to prevent spoofing with trivial lock scripts.
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
