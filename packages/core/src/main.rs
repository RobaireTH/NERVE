use axum::{
	routing::{get, post},
	Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

mod api;
mod ckb_client;
mod errors;
mod state;
mod tx_builder;

use api::{
	agent::{get_balance, get_cells},
	tx::{broadcast_tx, build_and_broadcast, build_tx, estimate_fee, tx_status},
};
use state::AppState;

#[tokio::main]
async fn main() {
	tracing_subscriber::fmt::init();

	let app_state = AppState::from_env().unwrap_or_else(|e| {
		tracing::warn!("could not load full AppState ({e}); starting in degraded mode");
		// Provide a no-op state so the service can still respond to /health on startup.
		panic!("AGENT_PRIVATE_KEY must be set: {e}");
	});

	let app = Router::new()
		.route("/health", get(health))
		// Agent identity endpoints.
		.route("/agent/balance", get(get_balance))
		.route("/agent/cells", get(get_cells))
		// Transaction builder endpoints.
		.route("/tx/build", post(build_tx))
		.route("/tx/broadcast", post(broadcast_tx))
		.route("/tx/build-and-broadcast", post(build_and_broadcast))
		.route("/tx/status", get(tx_status))
		.route("/tx/fee-rate", get(estimate_fee))
		.with_state(app_state)
		.layer(TraceLayer::new_for_http());

	let port: u16 = std::env::var("CORE_PORT")
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(8080);

	let addr = SocketAddr::from(([0, 0, 0, 0], port));
	let listener = TcpListener::bind(addr).await.unwrap();
	tracing::info!("nerve-core listening on {addr}");
	axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<Value> {
	Json(json!({ "status": "ok", "service": "nerve-core" }))
}
