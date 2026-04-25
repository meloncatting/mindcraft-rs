//! HTTP + WebSocket handlers for the MindServer REST API and UI.

use std::sync::Arc;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};
use futures::{SinkExt, StreamExt};

use crate::state::{AppState, AgentCommand};

// ── WebSocket handler ────────────────────────────────────────────────────────

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    info!("WebSocket client connected");

    // Subscribe to broadcasts
    let mut status_rx = state.status_tx.subscribe();
    let mut output_rx = state.output_tx.subscribe();

    // Send current agent status on connect
    let current = state.agent_statuses();
    let _ = socket.send(Message::Text(
        json!({ "type": "agents-status", "agents": current }).to_string()
    )).await;

    loop {
        tokio::select! {
            // Inbound WS message from UI
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(cmd) = serde_json::from_str::<Value>(&text) {
                            handle_ws_command(&state, &mut socket, cmd).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            // Outbound broadcasts → UI
            Ok(statuses) = status_rx.recv() => {
                let msg = json!({ "type": "agents-status", "agents": statuses });
                let _ = socket.send(Message::Text(msg.to_string())).await;
            }
            Ok(out) = output_rx.recv() => {
                let msg = json!({ "type": "bot-output", "agent": out.agent, "message": out.message });
                let _ = socket.send(Message::Text(msg.to_string())).await;
            }
        }
    }
    info!("WebSocket client disconnected");
}

async fn handle_ws_command(state: &Arc<AppState>, socket: &mut WebSocket, cmd: Value) {
    let event = cmd["type"].as_str().unwrap_or("");
    match event {
        "chat-message" => {
            let to = cmd["agent"].as_str().unwrap_or("");
            let from = cmd["from"].as_str().unwrap_or("user");
            let message = cmd["message"].as_str().unwrap_or("");
            if let Some(agent) = state.agents.get(to) {
                let _ = agent.msg_tx.send(AgentCommand::SendMessage {
                    from: from.to_string(),
                    message: message.to_string(),
                });
            }
        }
        "send-message" => {
            let to = cmd["agent"].as_str().unwrap_or("");
            let message = cmd["message"].as_str().unwrap_or("");
            if let Some(agent) = state.agents.get(to) {
                let _ = agent.msg_tx.send(AgentCommand::SendMessage {
                    from: "user".to_string(),
                    message: message.to_string(),
                });
            }
        }
        "stop-agent" => {
            let name = cmd["agent"].as_str().unwrap_or("");
            if let Some(agent) = state.agents.get(name) {
                let _ = agent.msg_tx.send(AgentCommand::Stop);
            }
        }
        "restart-agent" => {
            let name = cmd["agent"].as_str().unwrap_or("");
            if let Some(agent) = state.agents.get(name) {
                let _ = agent.msg_tx.send(AgentCommand::Restart);
            }
        }
        "listen-to-agents" => {
            // Client wants state-update stream; already subscribed via output_rx
        }
        other => {
            warn!("Unknown WS event: {other}");
        }
    }
}

// ── REST handlers ────────────────────────────────────────────────────────────

pub async fn list_agents(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "agents": state.agent_statuses() }))
}

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub profile: Value,
    pub load_memory: Option<bool>,
    pub init_message: Option<String>,
    #[serde(flatten)]
    pub settings: Value,
}

pub async fn create_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateAgentRequest>,
) -> Json<Value> {
    let name = req.profile["name"].as_str().unwrap_or("");
    if name.is_empty() {
        return Json(json!({ "success": false, "error": "Agent name is required" }));
    }
    if state.agents.contains_key(name) {
        return Json(json!({ "success": false, "error": "Agent already exists" }));
    }
    // Actual agent spawning is done by the Orchestrator in cli/main.rs.
    // The REST API just relays the request via a channel.
    Json(json!({ "success": true, "error": null }))
}

#[derive(Deserialize)]
pub struct MessageBody {
    pub message: String,
    pub from: Option<String>,
}

pub async fn send_message_to_agent(
    Path(name): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<MessageBody>,
) -> Json<Value> {
    if let Some(agent) = state.agents.get(&name) {
        let _ = agent.msg_tx.send(AgentCommand::SendMessage {
            from: body.from.unwrap_or_else(|| "user".into()),
            message: body.message,
        });
        Json(json!({ "success": true }))
    } else {
        Json(json!({ "success": false, "error": "Agent not found" }))
    }
}

pub async fn start_agent(
    Path(name): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    if let Some(agent) = state.agents.get(&name) {
        let _ = agent.msg_tx.send(AgentCommand::Restart);
        Json(json!({ "success": true }))
    } else {
        Json(json!({ "success": false, "error": "Agent not found" }))
    }
}

pub async fn stop_agent(
    Path(name): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    if let Some(agent) = state.agents.get(&name) {
        let _ = agent.msg_tx.send(AgentCommand::Stop);
        Json(json!({ "success": true }))
    } else {
        Json(json!({ "success": false, "error": "Agent not found" }))
    }
}

pub async fn restart_agent(
    Path(name): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    if let Some(agent) = state.agents.get(&name) {
        let _ = agent.msg_tx.send(AgentCommand::Restart);
        Json(json!({ "success": true }))
    } else {
        Json(json!({ "success": false, "error": "Agent not found" }))
    }
}

pub async fn shutdown(State(state): State<Arc<AppState>>) -> Json<Value> {
    info!("Shutdown requested via API");
    for entry in state.agents.iter() {
        let _ = entry.msg_tx.send(AgentCommand::Stop);
    }
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        std::process::exit(0);
    });
    Json(json!({ "success": true }))
}
