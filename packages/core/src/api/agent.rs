use axum::{
	extract::{Path, State},
	Json,
};
use serde_json::{json, Value};

use crate::{
	ckb_client::Script,
	errors::TxBuildError,
	state::{parse_capacity_hex, shannons_to_ckb, AppState, SECP256K1_CODE_HASH, SECP256K1_HASH_TYPE},
};

pub async fn get_balance(
	State(state): State<AppState>,
) -> Result<Json<Value>, TxBuildError> {
	let lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	let cells = state.ckb.get_cells_by_lock(&lock, 200).await?;

	let total_shannons: u64 = cells
		.objects
		.iter()
		.map(|c| parse_capacity_hex(&c.output.capacity).unwrap_or(0))
		.sum();

	Ok(Json(json!({
		"lock_args": state.lock_args,
		"balance_ckb": shannons_to_ckb(total_shannons),
		"balance_shannons": total_shannons,
		"cell_count": cells.objects.len(),
	})))
}

pub async fn get_cells(
	State(state): State<AppState>,
) -> Result<Json<Value>, TxBuildError> {
	let lock = Script {
		code_hash: SECP256K1_CODE_HASH.into(),
		hash_type: SECP256K1_HASH_TYPE.into(),
		args: state.lock_args.clone(),
	};

	let result = state.ckb.get_cells_by_lock(&lock, 100).await?;

	let cells: Vec<Value> = result
		.objects
		.iter()
		.map(|c| {
			json!({
				"out_point": c.out_point,
				"capacity_ckb": shannons_to_ckb(parse_capacity_hex(&c.output.capacity).unwrap_or(0)),
				"block_number": c.block_number,
				"type_script": c.output.type_script,
			})
		})
		.collect();

	Ok(Json(json!({ "cells": cells })))
}

/// GET /agent/sub-agents: list all managed sub-agents.
pub async fn list_sub_agents(
	State(state): State<AppState>,
) -> Result<Json<Value>, TxBuildError> {
	let agents = state.sub_agents.read().await;
	let list: Vec<Value> = agents
		.values()
		.map(|info| {
			json!({
				"lock_args": info.lock_args,
				"parent_lock_args": info.parent_lock_args,
				"revenue_share_bps": info.revenue_share_bps,
				"identity_outpoint": info.identity_outpoint,
			})
		})
		.collect();
	Ok(Json(json!({ "sub_agents": list, "count": list.len() })))
}

/// GET /agent/sub-agents/:lock_args: get a specific sub-agent.
pub async fn get_sub_agent(
	State(state): State<AppState>,
	Path(lock_args): Path<String>,
) -> Result<Json<Value>, TxBuildError> {
	let agents = state.sub_agents.read().await;
	let info = agents.get(&lock_args).ok_or_else(|| {
		TxBuildError::SubAgentError(format!("no sub-agent found for lock_args {lock_args}"))
	})?;
	Ok(Json(json!({
		"lock_args": info.lock_args,
		"parent_lock_args": info.parent_lock_args,
		"revenue_share_bps": info.revenue_share_bps,
		"identity_outpoint": info.identity_outpoint,
	})))
}
