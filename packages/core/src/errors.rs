use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use thiserror::Error;

// Variants are the full public API surface; not all are constructed in Day 2.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum TxBuildError {
	#[error("insufficient capacity: need {need} shannons, have {have}")]
	InsufficientCapacity { need: u64, have: u64 },

	#[error("invalid lock args: {0}")]
	InvalidLockArgs(String),

	#[error("missing cell dep: {0}")]
	MissingCellDep(String),

	#[error("cell not found: {0}")]
	CellNotFound(String),

	#[error("spending limit exceeded: requested {requested} shannons, limit {limit}")]
	SpendingLimitExceeded { requested: u64, limit: u64 },

	#[error("invalid type args: {0}")]
	InvalidTypeArgs(String),

	#[error("insufficient funds: need {need} CKB, have {have} CKB")]
	InsufficientFunds { need: f64, have: f64 },

	#[error("RPC error: {0}")]
	Rpc(String),

	#[error("signing error: {0}")]
	Signing(String),

	#[error("invalid address: {0}")]
	InvalidAddress(String),

	#[error("unknown intent: {0}")]
	UnknownIntent(String),

	#[error("sub-agent error: {0}")]
	SubAgentError(String),

	#[error("key store error: {0}")]
	KeyStoreError(String),
}

impl IntoResponse for TxBuildError {
	fn into_response(self) -> axum::response::Response {
		let (status, code) = match &self {
			TxBuildError::InsufficientCapacity { .. } => (StatusCode::BAD_REQUEST, "insufficient_capacity"),
			TxBuildError::InvalidLockArgs(_) => (StatusCode::BAD_REQUEST, "invalid_lock_args"),
			TxBuildError::MissingCellDep(_) => (StatusCode::BAD_REQUEST, "missing_cell_dep"),
			TxBuildError::CellNotFound(_) => (StatusCode::NOT_FOUND, "cell_not_found"),
			TxBuildError::SpendingLimitExceeded { .. } => (StatusCode::FORBIDDEN, "spending_limit_exceeded"),
			TxBuildError::InvalidTypeArgs(_) => (StatusCode::BAD_REQUEST, "invalid_type_args"),
			TxBuildError::InsufficientFunds { .. } => (StatusCode::BAD_REQUEST, "insufficient_funds"),
			TxBuildError::Rpc(_) => (StatusCode::BAD_GATEWAY, "rpc_error"),
			TxBuildError::Signing(_) => (StatusCode::INTERNAL_SERVER_ERROR, "signing_error"),
			TxBuildError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, "invalid_address"),
			TxBuildError::UnknownIntent(_) => (StatusCode::BAD_REQUEST, "unknown_intent"),
			TxBuildError::SubAgentError(_) => (StatusCode::BAD_REQUEST, "sub_agent_error"),
			TxBuildError::KeyStoreError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "key_store_error"),
		};

		(status, Json(json!({ "error": code, "message": self.to_string() }))).into_response()
	}
}
