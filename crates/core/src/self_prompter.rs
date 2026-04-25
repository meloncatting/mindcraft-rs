//! Autonomous goal loop (self-prompting).
//! Mirrors src/agent/self_prompter.js.

use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum SelfPrompterState {
    Stopped = 0,
    Active  = 1,
    Paused  = 2,
}

/// Messages sent from the self-prompter loop to the agent.
pub enum SelfPromptMsg {
    Message(String),
    Stop,
}

pub struct SelfPrompter {
    pub state: Mutex<SelfPrompterState>,
    pub prompt: Mutex<String>,
    interrupt: Mutex<bool>,
    loop_active: Mutex<bool>,
    cooldown_ms: u64,

    /// Channel to send messages to the agent's handleMessage
    msg_tx: mpsc::UnboundedSender<SelfPromptMsg>,
}

impl SelfPrompter {
    pub fn new(cooldown_ms: u64, msg_tx: mpsc::UnboundedSender<SelfPromptMsg>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(SelfPrompterState::Stopped),
            prompt: Mutex::new(String::new()),
            interrupt: Mutex::new(false),
            loop_active: Mutex::new(false),
            cooldown_ms,
            msg_tx,
        })
    }

    pub async fn is_active(&self) -> bool {
        *self.state.lock().await == SelfPrompterState::Active
    }

    pub async fn is_stopped(&self) -> bool {
        *self.state.lock().await == SelfPrompterState::Stopped
    }

    pub async fn is_paused(&self) -> bool {
        *self.state.lock().await == SelfPrompterState::Paused
    }

    pub async fn current_prompt(&self) -> String {
        self.prompt.lock().await.clone()
    }

    pub async fn start(self: Arc<Self>, prompt: Option<String>) -> Option<String> {
        let p = {
            let mut stored = self.prompt.lock().await;
            if let Some(new_p) = prompt {
                if new_p.is_empty() && stored.is_empty() {
                    return Some("No prompt specified. Ignoring request.".to_string());
                }
                if !new_p.is_empty() {
                    *stored = new_p;
                }
            } else if stored.is_empty() {
                return Some("No prompt specified. Ignoring request.".to_string());
            }
            stored.clone()
        };

        *self.state.lock().await = SelfPrompterState::Active;
        let sp = self.clone();
        tokio::spawn(async move { sp.run_loop().await });
        None
    }

    async fn run_loop(self: Arc<Self>) {
        if *self.loop_active.lock().await {
            warn!("Self-prompt loop already active");
            return;
        }
        *self.loop_active.lock().await = true;
        info!("Self-prompt loop started");

        let mut no_cmd_count = 0u32;
        const MAX_NO_CMD: u32 = 3;

        loop {
            if *self.interrupt.lock().await { break; }

            let goal = self.prompt.lock().await.clone();
            let msg = format!(
                "You are self-prompting with the goal: '{}'. \
                Your next response MUST contain a command with this syntax: !commandName. Respond:",
                goal
            );

            // Signal the agent to handle this message.
            // The agent will reply back via a oneshot or bool channel.
            // For simplicity we use a fire-and-wait pattern via the message channel.
            let _ = self.msg_tx.send(SelfPromptMsg::Message(msg));

            // Wait cooldown
            tokio::time::sleep(std::time::Duration::from_millis(self.cooldown_ms)).await;

            if *self.interrupt.lock().await { break; }
        }

        *self.loop_active.lock().await = false;
        *self.interrupt.lock().await = false;
        info!("Self-prompt loop stopped");
    }

    pub async fn stop(&self, stop_action: bool) {
        *self.interrupt.lock().await = true;
        // Caller responsible for stopping actions if stop_action=true
        self.stop_loop().await;
        *self.state.lock().await = SelfPrompterState::Stopped;
    }

    pub async fn pause(&self) {
        *self.interrupt.lock().await = true;
        self.stop_loop().await;
        *self.state.lock().await = SelfPrompterState::Paused;
    }

    async fn stop_loop(&self) {
        *self.interrupt.lock().await = true;
        // Spin-wait until loop exits
        while *self.loop_active.lock().await {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        *self.interrupt.lock().await = false;
    }

    pub async fn set_prompt_paused(&self, prompt: String) {
        *self.prompt.lock().await = prompt;
        *self.state.lock().await = SelfPrompterState::Paused;
    }

    pub async fn should_interrupt(&self, is_self_prompt: bool) -> bool {
        let state = *self.state.lock().await;
        is_self_prompt
            && (state == SelfPrompterState::Active || state == SelfPrompterState::Paused)
            && *self.interrupt.lock().await
    }

    pub async fn handle_user_prompted_cmd(&self, is_self_prompt: bool, is_action: bool) {
        if !is_self_prompt && is_action {
            self.stop_loop().await;
        }
    }
}
