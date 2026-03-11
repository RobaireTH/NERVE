use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::{
	errors::TxBuildError,
	state::{parse_capacity_hex, shannons_to_ckb, AppState, SECP256K1_CODE_HASH, SECP256K1_HASH_TYPE},
	ckb_client::Script,
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
