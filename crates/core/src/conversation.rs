//! Multi-agent conversation routing.
//! Mirrors src/agent/conversation.js.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{info, warn};

const WAIT_TIME_START_MS: u64 = 30_000;

#[derive(Debug, Default)]
struct Conversation {
    active: bool,
    ignore_until_start: bool,
    in_queue: Vec<QueuedMessage>,
}

#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub message: String,
    pub start: bool,
}

impl Conversation {
    fn reset(&mut self) {
        self.active = false;
        self.ignore_until_start = false;
        self.in_queue.clear();
    }

    fn end(&mut self) -> Option<String> {
        self.active = false;
        self.ignore_until_start = true;
        let full = compile_queued(&self.in_queue);
        self.in_queue.clear();
        if full.trim().is_empty() { None } else { Some(full) }
    }

    fn queue(&mut self, msg: QueuedMessage) {
        self.in_queue.push(msg);
    }
}

fn compile_queued(queue: &[QueuedMessage]) -> String {
    queue.iter()
        .map(|m| m.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct ConversationManager {
    agent_name: Mutex<String>,
    known_agents: Mutex<Vec<String>>,
    agents_in_game: Mutex<Vec<String>>,
    convos: Mutex<HashMap<String, Conversation>>,
    active_convo: Mutex<Option<String>>,
    awaiting_response: Mutex<bool>,
    wait_time_limit_ms: u64,
}

impl ConversationManager {
    pub fn new() -> Self {
        Self {
            agent_name: Mutex::new(String::new()),
            known_agents: Mutex::new(Vec::new()),
            agents_in_game: Mutex::new(Vec::new()),
            convos: Mutex::new(HashMap::new()),
            active_convo: Mutex::new(None),
            awaiting_response: Mutex::new(false),
            wait_time_limit_ms: WAIT_TIME_START_MS,
        }
    }

    pub async fn init(&self, agent_name: String) {
        *self.agent_name.lock().await = agent_name;
    }

    pub async fn register_agents(&self, names: Vec<String>) {
        *self.known_agents.lock().await = names;
    }

    pub async fn set_agents_in_game(&self, names: Vec<String>) {
        *self.agents_in_game.lock().await = names;
    }

    pub async fn is_other_agent(&self, name: &str) -> bool {
        self.known_agents.lock().await.contains(&name.to_string())
    }

    pub async fn other_agent_in_game(&self, name: &str) -> bool {
        self.agents_in_game.lock().await.contains(&name.to_string())
    }

    pub async fn in_conversation(&self, name: &str) -> bool {
        self.convos.lock().await
            .get(name)
            .map(|c| c.active)
            .unwrap_or(false)
    }

    pub async fn num_other_agents(&self) -> usize {
        self.known_agents.lock().await.len()
    }

    pub async fn response_scheduled_for(&self, name: &str) -> bool {
        let active = self.active_convo.lock().await;
        active.as_deref().map(|a| a != name).unwrap_or(false)
    }

    /// Receive a message from another bot. Returns (should_respond, compiled_message).
    pub async fn receive_from_bot(
        &self,
        from: &str,
        msg: QueuedMessage,
    ) -> Option<(bool, String)> {
        let mut convos = self.convos.lock().await;
        let convo = convos.entry(from.to_string()).or_default();

        if msg.start {
            convo.reset();
            convo.active = true;
            *self.active_convo.lock().await = Some(from.to_string());
        }

        if convo.ignore_until_start && !msg.start {
            return None;
        }

        convo.queue(msg.clone());

        if convo.active {
            let compiled = compile_queued(&convo.in_queue);
            convo.in_queue.clear();
            Some((true, compiled))
        } else {
            None
        }
    }

    pub async fn send_to_bot(&self, to: &str, message: String) -> Option<QueuedMessage> {
        // Returns the message package to be sent over the agent IPC channel.
        Some(QueuedMessage { message, start: false })
    }

    pub async fn end_all_conversations(&self) {
        let mut convos = self.convos.lock().await;
        for convo in convos.values_mut() {
            convo.end();
        }
        *self.active_convo.lock().await = None;
    }
}

impl Default for ConversationManager {
    fn default() -> Self {
        Self::new()
    }
}
