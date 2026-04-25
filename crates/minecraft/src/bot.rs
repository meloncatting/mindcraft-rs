//! BotHandle: high-level async facade over the Minecraft client.
//!
//! Production implementation: wrap azalea::Client.
//! Stub implementation (default feature): in-memory no-op for unit tests.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

use mindcraft_config::{Settings, AuthMode};

/// Events emitted by the bot that the agent loop needs to react to.
#[derive(Debug, Clone)]
pub enum BotEvent {
    Spawned,
    Died { message: String, position: [f64; 3] },
    Kicked { reason: String },
    Disconnected { reason: String },
    Chat { username: String, message: String },
    Whisper { username: String, message: String },
    Idle,
    HealthChanged { health: f32 },
}

/// Configuration for creating a bot connection.
#[derive(Debug, Clone)]
pub struct BotConfig {
    pub username: String,
    pub host: String,
    pub port: u16,
    pub auth: AuthMode,
    pub minecraft_version: Option<String>,
}

impl BotConfig {
    pub fn from_settings(username: String, s: &Settings) -> Self {
        Self {
            username,
            host: s.host.clone(),
            port: s.port.max(0) as u16,
            auth: s.auth.clone(),
            minecraft_version: if s.minecraft_version == "auto" {
                None
            } else {
                Some(s.minecraft_version.clone())
            },
        }
    }
}

/// High-level bot handle. All methods are async and cancel-safe.
pub struct BotHandle {
    config: BotConfig,
    event_tx: mpsc::UnboundedSender<BotEvent>,
    pub event_rx: RwLock<mpsc::UnboundedReceiver<BotEvent>>,

    /// Outbound chat queue
    chat_tx: mpsc::UnboundedSender<String>,

    /// Output text accumulated during action execution
    pub output_buffer: RwLock<String>,
    pub interrupt_code: RwLock<bool>,

    /// Command registry handle (set after bot starts)
    command_registry: RwLock<Option<Arc<mindcraft_commands::registry::CommandRegistry>>>,

    /// Server output channel (to MindServer)
    server_tx: mpsc::UnboundedSender<(String, String)>,
    /// Other-agent message channel
    agent_msg_tx: mpsc::UnboundedSender<(String, String)>,
}

impl BotHandle {
    pub fn new(
        config: BotConfig,
        server_tx: mpsc::UnboundedSender<(String, String)>,
        agent_msg_tx: mpsc::UnboundedSender<(String, String)>,
    ) -> Arc<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (chat_tx, _chat_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            config,
            event_tx,
            event_rx: RwLock::new(event_rx),
            chat_tx,
            output_buffer: RwLock::new(String::new()),
            interrupt_code: RwLock::new(false),
            command_registry: RwLock::new(None),
            server_tx,
            agent_msg_tx,
        })
    }

    pub fn set_command_registry(&self, r: Arc<mindcraft_commands::registry::CommandRegistry>) {
        // Sync set (called before run)
        if let Ok(mut guard) = self.command_registry.try_write() {
            *guard = Some(r);
        }
    }

    /// Connect to the Minecraft server and start the event loop.
    /// In the azalea impl, this calls azalea::ClientBuilder::new().
    pub async fn connect(&self) -> Result<()> {
        info!(
            "Connecting to {}:{} as {} (version: {:?})",
            self.config.host, self.config.port,
            self.config.username, self.config.minecraft_version
        );

        #[cfg(feature = "stub")]
        {
            // Stub: immediately send a Spawned event
            let _ = self.event_tx.send(BotEvent::Spawned);
            info!("Stub bot spawned.");
            return Ok(());
        }

        // Real azalea implementation (enabled with --features azalea):
        // use azalea::prelude::*;
        // let client = azalea::ClientBuilder::new()
        //     .set_handler(handler)
        //     .start(
        //         azalea::Account::offline(&self.config.username),
        //         format!("{}:{}", self.config.host, self.config.port)
        //     ).await?;
        // ...

        Ok(())
    }

    pub async fn chat(&self, message: &str) {
        let _ = self.chat_tx.send(message.to_string());
        info!("[{}] chat: {message}", self.config.username);
    }

    pub async fn whisper(&self, to: &str, message: &str) {
        self.chat(&format!("/w {to} {message}")).await;
    }

    pub async fn get_stats(&self) -> String {
        // In azalea impl: read client.component::<Health>(), Position, etc.
        "Stats: [azalea integration pending]".to_string()
    }

    pub async fn get_inventory(&self) -> String {
        "Inventory: [azalea integration pending]".to_string()
    }

    pub async fn get_nearby_entities(&self) -> String {
        "Entities: [azalea integration pending]".to_string()
    }

    pub async fn get_nearby_blocks(&self) -> String {
        "Blocks: [azalea integration pending]".to_string()
    }

    pub async fn get_command_docs(&self, blocked: &[String]) -> String {
        if let Some(reg) = self.command_registry.read().await.as_ref() {
            reg.get_docs(blocked)
        } else {
            String::new()
        }
    }

    /// Execute a !command from a message string.
    pub async fn execute_command(&self, message: &str) -> Result<Option<String>> {
        let reg = self.command_registry.read().await;
        if let Some(r) = reg.as_ref() {
            // We need an AgentContext impl here. The bot is responsible for implementing it.
            // For now return a placeholder.
            warn!("execute_command: AgentContext plumbing not yet connected");
            Ok(Some("Command registry not connected to agent context yet.".to_string()))
        } else {
            anyhow::bail!("Command registry not initialized")
        }
    }

    pub async fn send_output_to_server(&self, agent_name: &str, message: &str) {
        let _ = self.server_tx.send((agent_name.to_string(), message.to_string()));
    }

    pub async fn send_to_agent(&self, agent_name: &str, message: &str) {
        let _ = self.agent_msg_tx.send((agent_name.to_string(), message.to_string()));
    }

    pub async fn stop_pathfinder(&self) {
        // azalea: client.stop_pathfinding();
    }

    pub async fn stop_pvp(&self) {
        // azalea: pvp plugin stop
    }

    pub async fn emit_event(&self, event: BotEvent) {
        let _ = self.event_tx.send(event);
    }

    pub async fn health(&self) -> f32 {
        // azalea: client.component::<Health>().health
        20.0
    }

    pub async fn position(&self) -> [f64; 3] {
        // azalea: client.component::<Position>()
        [0.0, 64.0, 0.0]
    }
}

// ── Skill runner: wraps action execution ──────────────────────────────────

/// Skills are async functions that operate on a BotHandle.
/// They append output to output_buffer and check interrupt_code.
pub type SkillFn = Box<dyn Fn(Arc<BotHandle>) -> futures::future::BoxFuture<'static, Result<()>> + Send + Sync>;
