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
	admin::{deploy_bin, test_spending_cap},
	agent::{get_balance, get_cells, get_sub_agent, list_sub_agents},
	tx::{broadcast_tx, build_and_broadcast, build_tx, estimate_fee, tx_status},
};
use state::AppState;

#[tokio::main]
async fn main() {
	tracing_subscriber::fmt::init();

	let app_state = AppState::from_env().unwrap_or_else(|e| {
		eprintln!("fatal: {e}");
		std::process::exit(1);
	});

	let app = Router::new()
		.route("/health", get(health))
		// Agent identity endpoints.
		.route("/agent/balance", get(get_balance))
		.route("/agent/cells", get(get_cells))
		.route("/agent/sub-agents", get(list_sub_agents))
		.route("/agent/sub-agents/:lock_args", get(get_sub_agent))
		// Admin endpoints (deployment tooling, not exposed externally in production).
		.route("/admin/deploy-bin", post(deploy_bin))
		.route("/admin/test-spending-cap", post(test_spending_cap))
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
