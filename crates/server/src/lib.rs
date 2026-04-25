//! MindServer: central hub for all agents + web UI.
//!
//! JS used Express + Socket.IO with a separate process per agent.
//! Rust uses axum WebSocket + tokio channels; agents run as tokio tasks
//! in the same process, so no inter-process sockets are needed.
//!
//! The WebSocket API surface is kept compatible with the original
//! Socket.IO events so the existing web UI (index.html) still works.

pub mod handlers;
pub mod state;

use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
    extract::State,
};
use tower_http::services::ServeDir;
use tracing::info;

use state::AppState;

pub async fn run_server(port: u16, static_dir: &str, state: Arc<AppState>) -> Result<()> {
    let app = Router::new()
        // WebSocket endpoint (replaces Socket.IO)
        .route("/ws", get(handlers::ws_handler))
        // REST API
        .route("/api/agents", get(handlers::list_agents))
        .route("/api/agents", post(handlers::create_agent))
        .route("/api/agents/:name/message", post(handlers::send_message_to_agent))
        .route("/api/agents/:name/start",   post(handlers::start_agent))
        .route("/api/agents/:name/stop",    post(handlers::stop_agent))
        .route("/api/agents/:name/restart", post(handlers::restart_agent))
        .route("/api/shutdown",             post(handlers::shutdown))
        // Static web UI
        .nest_service("/", ServeDir::new(static_dir))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("MindServer running at http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
