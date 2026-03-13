// Job Cell Type Script
//
// Enforces the job marketplace state machine at CKB consensus level.
//
// State machine (strict adjacent transitions only):
//   Open (0) → Reserved (1): worker assigned, worker_lock_args must be set
//   Reserved (1) → Claimed (2): worker begins execution
//   Claimed (2) → Completed (3): work done, cell can be settled
//   Any → Expired (4): poster cancels
//
// Invariants:
//   - Status can only advance by exactly one step, or jump to Expired (4).
//   - poster_lock_args, reward, capability_hash, and ttl are immutable after creation.
//   - On Open→Reserved, worker_lock_args must be non-zero.
//   - Cell destruction (settlement) is always allowed — the lock script controls access.
//   - Creation: status=Open, poster_lock_args non-zero, reward > 0.
//
// Cell data layout (90 bytes minimum, little-endian):
//   [0]       version: u8         = 0
//   [1]       status: u8          0=Open 1=Reserved 2=Claimed 3=Completed 4=Expired
//   [2..22]   poster_lock_args:   [u8; 20]
//   [22..42]  worker_lock_args:   [u8; 20]  (zeros until reserved)
//   [42..50]  reward_shannons:    u64 LE
//   [50..58]  ttl_block_height:   u64 LE
//   [58..90]  capability_hash:    [u8; 32]

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
const ERR_INVALID_STATUS: i8 = 3;
const ERR_IMMUTABLE_FIELD_CHANGED: i8 = 4;
const ERR_ZERO_REWARD: i8 = 5;
const ERR_ZERO_POSTER: i8 = 6;
const ERR_ZERO_WORKER: i8 = 7;

const DATA_MIN: usize = 90;

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

	// Cell destruction (settlement/cancellation) is always allowed.
	let new = match load_cell_data(0, Source::GroupOutput) {
		Ok(d) => d,
		Err(SysError::IndexOutOfBound) => return Ok(()),
		Err(e) => return Err(sys_err(e)),
	};

	if new.len() < DATA_MIN {
		return Err(ERR_INVALID_DATA);
	}

	let old_status = old[1];
	let new_status = new[1];

	// Enforce strict state transitions: only adjacent steps or jump to Expired.
	let valid_transition = match (old_status, new_status) {
		(STATUS_OPEN, STATUS_RESERVED) => true,
		(STATUS_RESERVED, STATUS_CLAIMED) => true,
		(STATUS_CLAIMED, STATUS_COMPLETED) => true,
		(_, STATUS_EXPIRED) => true,
		_ => false,
	};
	if !valid_transition {
		return Err(ERR_INVALID_STATUS);
	}

	// When reserving, worker_lock_args must be non-zero.
	if old_status == STATUS_OPEN && new_status == STATUS_RESERVED {
		if new[22..42].iter().all(|&b| b == 0) {
			return Err(ERR_ZERO_WORKER);
		}
	}

	// Immutable fields: poster_lock_args, reward, ttl, capability_hash.
	if old[2..22] != new[2..22] {
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

	Ok(())
}

fn read_u64_le(bytes: &[u8]) -> Option<u64> {
	let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
	Some(u64::from_le_bytes(arr))
}
