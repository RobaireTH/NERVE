use axum::{extract::{rejection::JsonRejection, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
	errors::TxBuildError,
	state::AppState,
	tx_builder::intents::{build_and_sign, BuildRequest, BuildResult},
};

fn parse_build_request(body: Result<Json<BuildRequest>, JsonRejection>) -> Result<BuildRequest, TxBuildError> {
	body.map(|Json(req)| req)
		.map_err(|e| TxBuildError::UnknownIntent(e.body_text()))
}

/// POST /tx/build: build and sign a transaction without broadcasting.
pub async fn build_tx(
	State(state): State<AppState>,
	body: Result<Json<BuildRequest>, JsonRejection>,
) -> Result<Json<BuildResult>, TxBuildError> {
	let req = parse_build_request(body)?;
	let result = build_and_sign(&state, req).await?;
	Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct BroadcastRequest {
	pub tx: Value,
}

/// POST /tx/broadcast: broadcast a pre-built signed transaction.
pub async fn broadcast_tx(
	State(state): State<AppState>,
	Json(body): Json<BroadcastRequest>,
) -> Result<Json<Value>, TxBuildError> {
	let tx_hash = state.ckb.send_transaction(&body.tx).await?;
	Ok(Json(json!({ "tx_hash": tx_hash })))
}

/// POST /tx/build-and-broadcast: build, sign, and immediately broadcast.
pub async fn build_and_broadcast(
	State(state): State<AppState>,
	body: Result<Json<BuildRequest>, JsonRejection>,
) -> Result<Json<Value>, TxBuildError> {
	let req = parse_build_request(body)?;
	let result = build_and_sign(&state, req).await?;
	let tx_hash = state.ckb.send_transaction(&result.tx).await?;
	Ok(Json(json!({ "tx_hash": tx_hash })))
}

#[derive(Debug, Deserialize)]
pub struct TxStatusRequest {
	pub tx_hash: String,
}

/// GET /tx/status?tx_hash=0x...: fetch transaction status.
pub async fn tx_status(
	State(state): State<AppState>,
	axum::extract::Query(params): axum::extract::Query<TxStatusRequest>,
) -> Result<Json<Value>, TxBuildError> {
	let info = state.ckb.get_transaction(&params.tx_hash).await?;
	Ok(Json(info))
}

/// GET /tx/fee-rate: return the current estimated fee rate.
pub async fn estimate_fee(
	State(state): State<AppState>,
) -> Result<Json<Value>, TxBuildError> {
	let fee_rate = state.ckb.estimate_fee_rate().await?;
	Ok(Json(json!({
		"fee_rate_shannons_per_kb": fee_rate,
		"fee_rate_ckb_per_kb": fee_rate as f64 / 1e8,
	})))
}
