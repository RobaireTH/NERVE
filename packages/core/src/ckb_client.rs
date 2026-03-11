use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::TxBuildError;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Script {
	pub code_hash: String,
	pub hash_type: String,
	pub args: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutPoint {
	pub tx_hash: String,
	pub index: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CellOutput {
	pub capacity: String,
	pub lock: Script,
	#[serde(rename = "type")]
	pub type_script: Option<Script>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveCell {
	pub output: CellOutput,
	pub out_point: OutPoint,
	pub block_number: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetCellsResult {
	pub objects: Vec<LiveCell>,
	// Used for paginated queries in future iterations.
	#[allow(dead_code)]
	pub last_cursor: String,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
	result: Option<T>,
	error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
	message: String,
}

#[derive(Debug, Clone)]
pub struct CkbClient {
	http: Client,
	pub rpc_url: String,
	pub indexer_url: String,
}

impl CkbClient {
	pub fn new(rpc_url: String, indexer_url: String) -> Self {
		Self {
			http: Client::new(),
			rpc_url,
			indexer_url,
		}
	}

	async fn rpc<T: for<'de> Deserialize<'de>>(
		&self,
		method: &str,
		params: Value,
	) -> Result<T, TxBuildError> {
		let body = json!({
			"jsonrpc": "2.0",
			"id": 1,
			"method": method,
			"params": params,
		});

		let resp: RpcResponse<T> = self
			.http
			.post(&self.rpc_url)
			.json(&body)
			.send()
			.await
			.map_err(|e| TxBuildError::Rpc(e.to_string()))?
			.json()
			.await
			.map_err(|e| TxBuildError::Rpc(e.to_string()))?;

		if let Some(err) = resp.error {
			return Err(TxBuildError::Rpc(err.message));
		}
		resp.result.ok_or_else(|| TxBuildError::Rpc("empty result".into()))
	}

	async fn indexer<T: for<'de> Deserialize<'de>>(
		&self,
		method: &str,
		params: Value,
	) -> Result<T, TxBuildError> {
		let body = json!({
			"jsonrpc": "2.0",
			"id": 1,
			"method": method,
			"params": params,
		});

		let resp: RpcResponse<T> = self
			.http
			.post(&self.indexer_url)
			.json(&body)
			.send()
			.await
			.map_err(|e| TxBuildError::Rpc(e.to_string()))?
			.json()
			.await
			.map_err(|e| TxBuildError::Rpc(e.to_string()))?;

		if let Some(err) = resp.error {
			return Err(TxBuildError::Rpc(err.message));
		}
		resp.result.ok_or_else(|| TxBuildError::Rpc("empty result".into()))
	}

	#[allow(dead_code)]
	pub async fn get_tip_block_number(&self) -> Result<u64, TxBuildError> {
		let hex: String = self.rpc("get_tip_block_number", json!([])).await?;
		parse_hex_u64(&hex)
	}

	pub async fn get_cells_by_lock(
		&self,
		lock: &Script,
		limit: u32,
	) -> Result<GetCellsResult, TxBuildError> {
		let params = json!([
			{
				"script": {
					"code_hash": lock.code_hash,
					"hash_type": lock.hash_type,
					"args": lock.args,
				},
				"script_type": "lock",
			},
			"asc",
			format!("{:#x}", limit),
		]);
		self.indexer("get_cells", params).await
	}

	pub async fn send_transaction(&self, tx: &Value) -> Result<String, TxBuildError> {
		self.rpc("send_transaction", json!([tx, "passthrough"])).await
	}

	pub async fn test_tx_pool_accept(&self, tx: &Value) -> Result<Value, TxBuildError> {
		self.rpc("test_tx_pool_accept", json!([tx, "passthrough"])).await
	}

	pub async fn get_transaction(&self, tx_hash: &str) -> Result<Value, TxBuildError> {
		self.rpc("get_transaction", json!([tx_hash])).await
	}

	pub async fn estimate_fee_rate(&self) -> Result<u64, TxBuildError> {
		#[derive(Deserialize)]
		struct FeeRate {
			fee_rate: String,
		}
		let result: FeeRate = self.rpc("estimate_fee_rate", json!([null])).await?;
		parse_hex_u64(&result.fee_rate)
	}
}

pub fn parse_hex_u64(hex: &str) -> Result<u64, TxBuildError> {
	let stripped = hex.trim_start_matches("0x");
	u64::from_str_radix(stripped, 16).map_err(|e| TxBuildError::Rpc(format!("hex parse: {e}")))
}
