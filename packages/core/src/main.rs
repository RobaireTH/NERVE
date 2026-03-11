use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
	tracing_subscriber::fmt::init();

	let app = Router::new().route("/health", get(health));

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
