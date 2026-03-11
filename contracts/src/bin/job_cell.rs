// Job Cell Type Script
//
// Enforces the job marketplace state machine at CKB consensus level.
//
// State machine:
//   Open (0) → Reserved (1): anyone may set worker_lock_args
//   Reserved (1) → Claimed (2): only the worker (checked via lock script)
//   Claimed (2) → Completed (3): only the worker; reward is released
//   Claimed (2) → Expired (4): any tx after ttl_block_height (simplified: checked by poster)
//   Open/Reserved (0,1) → Expired (4): poster can reclaim after TTL
//
// Invariants enforced by this type script:
//   - Status can only advance (never decrease) or jump to Expired (4).
//   - poster_lock_args, reward, capability_hash, and ttl are immutable after creation.
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

const DATA_MIN: usize = 90;
const STATUS_EXPIRED: u8 = 4;

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
		validate_transition()
	}
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;
	if data.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }
	if data[0] != 0 { return Err(ERR_INVALID_DATA); }    // version
	if data[1] != 0 { return Err(ERR_INVALID_STATUS); }  // must start Open

	// poster_lock_args must be non-zero.
	if data[2..22].iter().all(|&b| b == 0) { return Err(ERR_ZERO_POSTER); }

	// reward must be > 0.
	let reward = read_u64_le(&data[42..50]).ok_or(ERR_INVALID_DATA)?;
	if reward == 0 { return Err(ERR_ZERO_REWARD); }

	Ok(())
}

fn validate_transition() -> Result<(), i8> {
	let old = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;
	let new = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;

	if old.len() < DATA_MIN || new.len() < DATA_MIN { return Err(ERR_INVALID_DATA); }

	let old_status = old[1];
	let new_status = new[1];

	// Status must advance or jump to Expired; never regress.
	if new_status == STATUS_EXPIRED {
		// Any state can transition to Expired (TTL enforcement is app-layer).
	} else if new_status <= old_status {
		return Err(ERR_INVALID_STATUS);
	}

	// Immutable fields: poster_lock_args, reward, ttl, capability_hash.
	if old[2..22] != new[2..22] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); }   // poster
	if old[42..50] != new[42..50] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); } // reward
	if old[50..58] != new[50..58] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); } // ttl
	if old[58..90] != new[58..90] { return Err(ERR_IMMUTABLE_FIELD_CHANGED); } // cap_hash

	Ok(())
}

fn read_u64_le(bytes: &[u8]) -> Option<u64> {
	let arr: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
	Some(u64::from_le_bytes(arr))
}
