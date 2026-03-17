// Reputation Type Script
//
// Tracks an agent's on-chain reputation through a dispute-windowed update protocol.
// Uses CKB Type ID pattern to guarantee one reputation cell per agent.
//
// Type script args layout:
//   [0..32]  type_id: [u8; 32]  (singleton guarantee)
//
// State machine:
//   Idle (pending_type=0) → Proposed (pending_type=1 or 2): propose a completed/abandoned update
//   Proposed → Finalized: increment counter, clear pending (requires dispute window elapsed)
//   Proposed → Disputed: clear pending without incrementing (dispute wins)
//
// Dispute window enforcement:
//   When finalizing (Proposed→Idle with counter increment), the consuming input's
//   `since` field must encode an absolute block number >= pending_expires_at. CKB
//   consensus already validates the since constraint, so the type script only needs
//   to verify the TX builder set the correct since value.
//
// Cell data layout V0 (46 bytes minimum, little-endian):
//   [0]       version: u8         = 0
//   [1]       pending_type: u8    0=none 1=propose_completed 2=propose_abandoned
//   [2..10]   jobs_completed: u64 LE
//   [10..18]  jobs_abandoned: u64 LE
//   [18..26]  pending_expires_at: u64 LE  (absolute block height; 0 if no pending)
//   [26..46]  agent_lock_args: [u8; 20]
//
// Cell data layout V1 (110 bytes, adds blake2b hash-chain provability):
//   [0..46]   same as V0 (version byte = 1)
//   [46..78]  proof_root: [u8; 32]              blake2b hash chain accumulator
//   [78..110] pending_settlement_hash: [u8; 32] evidence for current proposal

#![no_std]
#![no_main]
#![allow(unexpected_cfgs)]

use ckb_hash::blake2b_256;
use ckb_std::{
	ckb_constants::Source,
	default_alloc,
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

const DATA_MIN: usize = 46;
const DATA_V1: usize = 110;

// Since field: bits 63-61 must be 000 for absolute block number metric.
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
	// Enforce Type ID singleton.
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

/// Computes new_proof_root = blake2b(old_root || settlement_hash).
fn compute_new_proof_root(old_root: &[u8; 32], settlement_hash: &[u8; 32]) -> [u8; 32] {
	let mut preimage = [0u8; 64];
	preimage[..32].copy_from_slice(old_root);
	preimage[32..].copy_from_slice(settlement_hash);
	blake2b_256(preimage)
}

fn validate_creation() -> Result<(), i8> {
	let data = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;
	if data.len() < DATA_MIN {
		return Err(ERR_INVALID_DATA);
	}

	let version = data[0];
	if version > 1 {
		return Err(ERR_INVALID_DATA);
	}

	// Must start Idle.
	if data[1] != 0 {
		return Err(ERR_INVALID_TRANSITION);
	}

	// Counters must be 0 on creation.
	if read_u64_le(&data[2..10]).ok_or(ERR_INVALID_DATA)? != 0 {
		return Err(ERR_INVALID_DATA);
	}
	if read_u64_le(&data[10..18]).ok_or(ERR_INVALID_DATA)? != 0 {
		return Err(ERR_INVALID_DATA);
	}

	// pending_expires_at must be 0 (no pending proposal).
	if read_u64_le(&data[18..26]).ok_or(ERR_INVALID_DATA)? != 0 {
		return Err(ERR_INVALID_DATA);
	}

	// agent_lock_args must be non-zero.
	if is_all_zero(&data[26..46]) {
		return Err(ERR_ZERO_AGENT);
	}

	// V1: proof_root and pending_settlement_hash must be zero on creation.
	if version == 1 {
		if data.len() < DATA_V1 {
			return Err(ERR_INVALID_DATA);
		}
		if !is_all_zero(&data[46..78]) {
			return Err(ERR_INVALID_PROOF_ROOT);
		}
		if !is_all_zero(&data[78..110]) {
			return Err(ERR_INVALID_SETTLEMENT_HASH);
		}
	}

	Ok(())
}

fn validate_update() -> Result<(), i8> {
	let old = load_cell_data(0, Source::GroupInput).map_err(|_| ERR_INVALID_DATA)?;
	let new = load_cell_data(0, Source::GroupOutput).map_err(|_| ERR_INVALID_DATA)?;

	if old.len() < DATA_MIN || new.len() < DATA_MIN {
		return Err(ERR_INVALID_DATA);
	}

	// Immutable: agent_lock_args.
	if old[26..46] != new[26..46] {
		return Err(ERR_IMMUTABLE_FIELD);
	}

	let old_version = old[0];
	let new_version = new[0];

	// Version can only stay the same or migrate from 0 to 1.
	if new_version > 1 {
		return Err(ERR_INVALID_DATA);
	}

	// Detect V0→V1 migration: version changes 0→1 while staying Idle.
	if old_version == 0 && new_version == 1 {
		return validate_migration_v0_to_v1(&old, &new);
	}

	// Cannot downgrade version.
	if new_version < old_version {
		return Err(ERR_INVALID_DATA);
	}

	let old_pending = old[1];
	let new_pending = new[1];
	let old_completed = read_u64_le(&old[2..10]).ok_or(ERR_INVALID_DATA)?;
	let old_abandoned = read_u64_le(&old[10..18]).ok_or(ERR_INVALID_DATA)?;
	let new_completed = read_u64_le(&new[2..10]).ok_or(ERR_INVALID_DATA)?;
	let new_abandoned = read_u64_le(&new[10..18]).ok_or(ERR_INVALID_DATA)?;

	match (old_pending, new_pending) {
		// Idle → Proposed: counters must not change.
		(0, 1) | (0, 2) => {
			if new_completed != old_completed {
				return Err(ERR_WRONG_COUNTER);
			}
			if new_abandoned != old_abandoned {
				return Err(ERR_WRONG_COUNTER);
			}
			// pending_expires_at must be set (non-zero).
			let expires_at = read_u64_le(&new[18..26]).ok_or(ERR_INVALID_DATA)?;
			if expires_at == 0 {
				return Err(ERR_INVALID_TRANSITION);
			}

			// V1: settlement_hash must go from zero to non-zero; proof_root unchanged.
			if new_version == 1 {
				validate_v1_propose(&old, &new)?;
			}
		}
		// Proposed(completed) → Finalized: increment completed by 1.
		(1, 0) => {
			validate_dispute_window_elapsed(&old)?;

			if new_completed != old_completed + 1 {
				return Err(ERR_WRONG_COUNTER);
			}
			if new_abandoned != old_abandoned {
				return Err(ERR_WRONG_COUNTER);
			}
			validate_pending_cleared(&new)?;

			// V1: verify proof_root update and settlement_hash cleared.
			if new_version == 1 {
				validate_v1_finalize(&old, &new)?;
			}
		}
		// Proposed(abandoned) → Finalized: increment abandoned by 1.
		(2, 0) => {
			validate_dispute_window_elapsed(&old)?;

			if new_completed != old_completed {
				return Err(ERR_WRONG_COUNTER);
			}
			if new_abandoned != old_abandoned + 1 {
				return Err(ERR_WRONG_COUNTER);
			}
			validate_pending_cleared(&new)?;

			// V1: verify proof_root update and settlement_hash cleared.
			if new_version == 1 {
				validate_v1_finalize(&old, &new)?;
			}
		}
		_ => return Err(ERR_INVALID_TRANSITION),
	}

	Ok(())
}

/// Validates V0→V1 migration: counters preserved, proof fields zeroed, must be Idle.
fn validate_migration_v0_to_v1(old: &[u8], new: &[u8]) -> Result<(), i8> {
	if new.len() < DATA_V1 {
		return Err(ERR_INVALID_DATA);
	}

	// Must be Idle in both old and new.
	if old[1] != 0 || new[1] != 0 {
		return Err(ERR_INVALID_TRANSITION);
	}

	// Counters must be preserved exactly.
	if old[2..10] != new[2..10] || old[10..18] != new[10..18] {
		return Err(ERR_WRONG_COUNTER);
	}

	// pending_expires_at must remain 0.
	if read_u64_le(&new[18..26]).ok_or(ERR_INVALID_DATA)? != 0 {
		return Err(ERR_PENDING_NOT_CLEARED);
	}

	// proof_root and pending_settlement_hash must be zero.
	if !is_all_zero(&new[46..78]) {
		return Err(ERR_INVALID_PROOF_ROOT);
	}
	if !is_all_zero(&new[78..110]) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	Ok(())
}

/// V1 Idle→Proposed: settlement_hash must transition zero→non-zero; proof_root unchanged.
fn validate_v1_propose(old: &[u8], new: &[u8]) -> Result<(), i8> {
	if old.len() < DATA_V1 || new.len() < DATA_V1 {
		return Err(ERR_INVALID_DATA);
	}

	let old_proof_root = read_bytes_32(old, 46).ok_or(ERR_INVALID_DATA)?;
	let new_proof_root = read_bytes_32(new, 46).ok_or(ERR_INVALID_DATA)?;

	// proof_root must not change during proposal.
	if old_proof_root != new_proof_root {
		return Err(ERR_PROOF_ROOT_MISMATCH);
	}

	let old_settlement = read_bytes_32(old, 78).ok_or(ERR_INVALID_DATA)?;
	let new_settlement = read_bytes_32(new, 78).ok_or(ERR_INVALID_DATA)?;

	// Old settlement_hash must be zero (was cleared on last finalize or creation).
	if !is_all_zero(&old_settlement) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	// New settlement_hash must be non-zero (evidence of job completion).
	if is_all_zero(&new_settlement) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	Ok(())
}

/// V1 Proposed→Finalized: proof_root = blake2b(old_root || old_settlement); settlement cleared.
fn validate_v1_finalize(old: &[u8], new: &[u8]) -> Result<(), i8> {
	if old.len() < DATA_V1 || new.len() < DATA_V1 {
		return Err(ERR_INVALID_DATA);
	}

	let old_proof_root = read_bytes_32(old, 46).ok_or(ERR_INVALID_DATA)?;
	let old_settlement = read_bytes_32(old, 78).ok_or(ERR_INVALID_DATA)?;
	let new_proof_root = read_bytes_32(new, 46).ok_or(ERR_INVALID_DATA)?;
	let new_settlement = read_bytes_32(new, 78).ok_or(ERR_INVALID_DATA)?;

	// Compute expected new proof_root.
	let expected = compute_new_proof_root(&old_proof_root, &old_settlement);
	if new_proof_root != expected {
		return Err(ERR_PROOF_ROOT_MISMATCH);
	}

	// Settlement hash must be cleared after finalization.
	if !is_all_zero(&new_settlement) {
		return Err(ERR_INVALID_SETTLEMENT_HASH);
	}

	Ok(())
}

/// Verifies the dispute window has elapsed by checking the input's `since` field.
fn validate_dispute_window_elapsed(old_data: &[u8]) -> Result<(), i8> {
	let pending_expires_at = read_u64_le(&old_data[18..26]).ok_or(ERR_INVALID_DATA)?;
	if pending_expires_at == 0 {
		// No dispute window set — this shouldn't happen in Proposed state.
		return Err(ERR_INVALID_TRANSITION);
	}

	let since = load_input_since(0, Source::GroupInput).map_err(sys_err)?;

	// Since must be an absolute block number (bits 63-61 = 000).
	if since & SINCE_METRIC_MASK != 0 {
		return Err(ERR_DISPUTE_WINDOW_ACTIVE);
	}

	if since < pending_expires_at {
		return Err(ERR_DISPUTE_WINDOW_ACTIVE);
	}

	Ok(())
}

/// Verifies pending_expires_at is reset to 0 when returning to Idle.
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
