//! Shared server state. Replaces the in-memory agent_connections map
//! in mindserver.js. Thread-safe via DashMap + tokio channels.

use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::{mpsc, broadcast};
use serde::{Deserialize, Serialize};

/// Per-agent runtime record
pub struct AgentEntry {
    pub name: String,
    pub in_game: bool,
    pub viewer_port: u16,
    /// Channel to send messages to this agent
    pub msg_tx: mpsc::UnboundedSender<AgentCommand>,
    /// Last known full state snapshot
    pub full_state: Option<AgentState>,
}

/// Commands the server can issue to an agent task.
#[derive(Debug, Clone)]
pub enum AgentCommand {
    SendMessage { from: String, message: String },
    Restart,
    Stop,
    GetFullState { reply_json: String }, // serialized AgentState returned via separate channel
}

/// Agent status broadcast (replaces socket.io agents-status event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub name: String,
    pub in_game: bool,
    pub viewer_port: u16,
}

/// Full world state snapshot for the UI state-update event.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentState {
    pub name: String,
    pub health: f32,
    pub food: i32,
    pub position: Option<[f64; 3]>,
    pub inventory: Vec<InventorySlot>,
    pub nearby_blocks: Vec<String>,
    pub nearby_entities: Vec<String>,
    pub current_action: String,
    pub memory: String,
    pub self_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventorySlot {
    pub name: String,
    pub count: i32,
}

/// Bot output broadcast (replaces socket.io bot-output event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotOutput {
    pub agent: String,
    pub message: String,
}

pub struct AppState {
    /// agent_name → entry
    pub agents: DashMap<String, AgentEntry>,
    /// Broadcast to all WebSocket clients: agents status list
    pub status_tx: broadcast::Sender<Vec<AgentStatus>>,
    /// Broadcast bot output to UI listeners
    pub output_tx: broadcast::Sender<BotOutput>,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        let (status_tx, _) = broadcast::channel(64);
        let (output_tx, _) = broadcast::channel(1024);
        Arc::new(Self {
            agents: DashMap::new(),
            status_tx,
            output_tx,
        })
    }

    pub fn agent_statuses(&self) -> Vec<AgentStatus> {
        self.agents.iter().map(|e| AgentStatus {
            name: e.name.clone(),
            in_game: e.in_game,
            viewer_port: e.viewer_port,
        }).collect()
    }

    pub fn broadcast_status(&self) {
        let statuses = self.agent_statuses();
        let _ = self.status_tx.send(statuses);
    }

    pub fn broadcast_output(&self, agent: &str, message: &str) {
        let _ = self.output_tx.send(BotOutput {
            agent: agent.to_string(),
            message: message.to_string(),
        });
    }
}
