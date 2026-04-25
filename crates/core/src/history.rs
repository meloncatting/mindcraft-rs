//! Conversation history + LLM memory summarization.
//! Mirrors src/agent/history.js.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

use mindcraft_llm::{Turn, Role, TurnContent};

const MAX_MEMORY_CHARS: usize = 500;

#[derive(Debug, Serialize, Deserialize)]
pub struct HistorySnapshot {
    pub memory: String,
    pub turns: Vec<Turn>,
    pub self_prompting_state: u8, // 0=stopped, 1=active, 2=paused
    pub self_prompt: Option<String>,
    pub task_start: Option<u64>,
    pub last_sender: Option<String>,
}

pub struct History {
    pub agent_name: String,
    pub memory: String,
    pub turns: Vec<Turn>,

    max_messages: usize,
    summary_chunk_size: usize,

    memory_path: PathBuf,
    full_history_path: Option<PathBuf>,
    histories_dir: PathBuf,

    /// Callback to ask the LLM for a memory summary.
    /// We use a channel sender to avoid circular Arc references.
    summary_tx: tokio::sync::mpsc::UnboundedSender<(Vec<Turn>, tokio::sync::oneshot::Sender<String>)>,
}

impl History {
    pub fn new(
        agent_name: String,
        max_messages: usize,
        summary_tx: tokio::sync::mpsc::UnboundedSender<(Vec<Turn>, tokio::sync::oneshot::Sender<String>)>,
    ) -> Self {
        let bot_dir = PathBuf::from(format!("bots/{agent_name}"));
        let histories_dir = bot_dir.join("histories");
        std::fs::create_dir_all(&histories_dir).ok();

        Self {
            agent_name: agent_name.clone(),
            memory: String::new(),
            turns: Vec::new(),
            max_messages,
            summary_chunk_size: 5,
            memory_path: bot_dir.join("memory.json"),
            full_history_path: None,
            histories_dir,
            summary_tx,
        }
    }

    pub fn get_history(&self) -> Vec<Turn> {
        self.turns.clone()
    }

    pub async fn add(&mut self, source: &str, content: &str) {
        let (role, formatted_content) = if source == "system" {
            (Role::System, content.to_string())
        } else if source == self.agent_name {
            (Role::Assistant, content.to_string())
        } else {
            (Role::User, format!("{source}: {content}"))
        };

        self.turns.push(Turn {
            role,
            content: TurnContent::Text(formatted_content),
        });

        if self.turns.len() >= self.max_messages {
            self.compress_history().await;
        }
    }

    async fn compress_history(&mut self) {
        let mut chunk: Vec<Turn> = self.turns.drain(..self.summary_chunk_size).collect();
        // Ensure chunk doesn't end with an assistant message (API requirement)
        while self.turns.first().map(|t| &t.role) == Some(&Role::Assistant) {
            chunk.push(self.turns.remove(0));
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        if self.summary_tx.send((chunk.clone(), tx)).is_ok() {
            if let Ok(summary) = rx.await {
                let mut memory = summary;
                if memory.len() > MAX_MEMORY_CHARS {
                    memory.truncate(MAX_MEMORY_CHARS);
                    memory.push_str("...(Memory truncated)");
                }
                self.memory = memory;
                info!("Memory updated: {}", &self.memory);
            }
        }

        self.append_full_history(&chunk).await;
    }

    async fn append_full_history(&mut self, turns: &[Turn]) {
        if self.full_history_path.is_none() {
            let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
            self.full_history_path = Some(self.histories_dir.join(format!("{ts}.json")));
            if let Some(p) = &self.full_history_path {
                let _ = std::fs::write(p, "[]");
            }
        }
        if let Some(path) = &self.full_history_path {
            let existing = std::fs::read_to_string(path).unwrap_or_else(|_| "[]".to_string());
            let mut full: Vec<Turn> = serde_json::from_str(&existing).unwrap_or_default();
            full.extend_from_slice(turns);
            if let Ok(serialized) = serde_json::to_string_pretty(&full) {
                let _ = std::fs::write(path, serialized);
            }
        }
    }

    pub fn save(
        &self,
        self_prompting_state: u8,
        self_prompt: Option<&str>,
        task_start: Option<u64>,
        last_sender: Option<&str>,
    ) -> Result<()> {
        let snapshot = HistorySnapshot {
            memory: self.memory.clone(),
            turns: self.turns.clone(),
            self_prompting_state,
            self_prompt: self_prompt.map(str::to_string),
            task_start,
            last_sender: last_sender.map(str::to_string),
        };
        let json = serde_json::to_string_pretty(&snapshot)?;
        std::fs::write(&self.memory_path, json)?;
        info!("Saved memory to {:?}", self.memory_path);
        Ok(())
    }

    pub fn load(&mut self) -> Option<HistorySnapshot> {
        if !self.memory_path.exists() {
            info!("No memory file found.");
            return None;
        }
        match std::fs::read_to_string(&self.memory_path)
            .and_then(|s| Ok(serde_json::from_str::<HistorySnapshot>(&s)?))
        {
            Ok(snapshot) => {
                self.memory = snapshot.memory.clone();
                self.turns = snapshot.turns.clone();
                info!("Loaded memory: {}", &self.memory);
                Some(snapshot)
            }
            Err(e) => {
                warn!("Failed to load history: {e}");
                None
            }
        }
    }

    pub fn clear(&mut self) {
        self.turns.clear();
        self.memory.clear();
    }
}
