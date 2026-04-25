//! Core agent loop.
//! Mirrors src/agent/agent.js.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{error, info, warn};
use anyhow::Result;

use mindcraft_config::Settings;
use mindcraft_llm::{Turn, Role, TurnContent};
use mindcraft_commands::parser::{contains_command, trunc_command_message};
use mindcraft_minecraft::BotHandle;

use crate::{
    action_manager::ActionManager,
    history::History,
    self_prompter::{SelfPrompter, SelfPromptMsg, SelfPrompterState},
    conversation::ConversationManager,
    memory_bank::MemoryBank,
    modes::ModeManager,
};
use mindcraft_llm::prompter::{Prompter, PromptContext};

pub struct Agent {
    pub name: String,
    pub settings: Settings,

    pub prompter: Arc<Prompter>,
    pub history: Arc<Mutex<History>>,
    pub actions: Arc<ActionManager>,
    pub self_prompter: Arc<SelfPrompter>,
    pub convo_manager: Arc<ConversationManager>,
    pub memory_bank: Arc<Mutex<MemoryBank>>,
    pub modes: Arc<ModeManager>,
    pub bot: Arc<BotHandle>,

    /// Last bot that sent us a message (for routing replies)
    pub last_sender: Mutex<Option<String>>,
    pub shut_up: Mutex<bool>,

    /// Blocked action names
    pub blocked_actions: Vec<String>,

    /// Inbound messages for the agent loop
    msg_rx: Mutex<mpsc::UnboundedReceiver<InboundMsg>>,
    /// Clone handle to send messages to self
    msg_tx: mpsc::UnboundedSender<InboundMsg>,

    /// Self-prompter messages
    sp_rx: Mutex<mpsc::UnboundedReceiver<SelfPromptMsg>>,
}

#[derive(Debug)]
pub struct InboundMsg {
    pub source: String,
    pub content: String,
    pub max_responses: Option<i32>,
}

impl Agent {
    pub fn new(
        name: String,
        settings: Settings,
        prompter: Arc<Prompter>,
        bot: Arc<BotHandle>,
        blocked_actions: Vec<String>,
    ) -> Arc<Self> {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let (sp_tx, sp_rx) = mpsc::unbounded_channel();
        let (summary_tx, mut summary_rx) = mpsc::unbounded_channel::<(Vec<Turn>, tokio::sync::oneshot::Sender<String>)>();

        let max_messages = settings.max_messages;
        let history = Arc::new(Mutex::new(History::new(
            name.clone(),
            max_messages,
            summary_tx,
        )));

        // Spawn background task: handles memory summarization requests from History
        {
            let prompter_clone = prompter.clone();
            tokio::spawn(async move {
                while let Some((turns, reply_tx)) = summary_rx.recv().await {
                    let to_summarize = mindcraft_llm::prompter::stringify_turns(&turns);
                    match prompter_clone.prompt_mem_saving(&to_summarize).await {
                        Ok(s) => { let _ = reply_tx.send(s); }
                        Err(e) => { warn!("Memory summarization failed: {e}"); }
                    }
                }
            });
        }

        let self_prompter = SelfPrompter::new(2000, sp_tx);
        let convo_manager = Arc::new(ConversationManager::new());

        Arc::new(Self {
            name,
            settings,
            prompter,
            history,
            actions: ActionManager::new(),
            self_prompter,
            convo_manager,
            memory_bank: Arc::new(Mutex::new(MemoryBank::new())),
            modes: ModeManager::new(),
            bot,
            last_sender: Mutex::new(None),
            shut_up: Mutex::new(false),
            blocked_actions,
            msg_rx: Mutex::new(msg_rx),
            msg_tx,
            sp_rx: Mutex::new(sp_rx),
        })
    }

    /// Send a message to this agent from an external source.
    pub fn send_message(&self, source: impl Into<String>, content: impl Into<String>, max_responses: Option<i32>) {
        let _ = self.msg_tx.send(InboundMsg {
            source: source.into(),
            content: content.into(),
            max_responses,
        });
    }

    /// Main agent event loop. Run this as a tokio task.
    pub async fn run(self: Arc<Self>) {
        info!("{} agent loop started", self.name);

        // Emit initial idle
        self.emit_idle().await;

        let sp_loop = self.clone();
        tokio::spawn(async move {
            sp_loop.self_prompt_dispatch_loop().await;
        });

        // Main message loop
        loop {
            let msg = {
                let mut rx = self.msg_rx.lock().await;
                rx.recv().await
            };
            match msg {
                Some(m) => {
                    if let Err(e) = self.handle_message(&m.source, &m.content, m.max_responses).await {
                        error!("Error handling message: {e}");
                    }
                }
                None => break,
            }
        }
        info!("{} agent loop ended", self.name);
    }

    async fn self_prompt_dispatch_loop(self: Arc<Self>) {
        loop {
            let sp_msg = {
                let mut rx = self.sp_rx.lock().await;
                rx.recv().await
            };
            match sp_msg {
                Some(SelfPromptMsg::Message(msg)) => {
                    self.send_message("system", msg, Some(-1));
                }
                Some(SelfPromptMsg::Stop) | None => break,
            }
        }
    }

    pub async fn emit_idle(&self) {
        let is_idle = !self.actions.is_executing().await;
        if is_idle {
            // Modes react to idle; resume action if any
            // In the full impl, this triggers mode updates and resume
            self.modes.on_idle(&self.name).await;
        }
    }

    /// Core message handler. Returns whether a command was used.
    pub async fn handle_message(
        self: &Arc<Self>,
        source: &str,
        message: &str,
        max_responses: Option<i32>,
    ) -> Result<bool> {
        if source.is_empty() || message.is_empty() {
            warn!("Received empty message from {source}");
            return Ok(false);
        }

        let max_resp = max_responses.unwrap_or_else(|| {
            if self.settings.max_commands == -1 { i32::MAX } else { self.settings.max_commands }
        });
        let max_resp = if max_resp == -1 { i32::MAX } else { max_resp };

        let self_prompt = source == "system" || source == self.name;
        let from_other_bot = self.convo_manager.is_other_agent(source).await;

        // Handle direct user commands (bypass LLM)
        if !self_prompt && !from_other_bot {
            if let Some(cmd_name) = contains_command(message) {
                return self.handle_user_command(source, message, &cmd_name).await;
            }
        }

        if from_other_bot {
            *self.last_sender.lock().await = Some(source.to_string());
        }

        // Add to history
        {
            let mut hist = self.history.lock().await;
            hist.add(source, message).await;
        }
        self.save_history().await;

        // If self-prompting is active and user sends message, cap responses at 1
        let max_resp = if !self_prompt && self.self_prompter.is_active().await {
            1
        } else {
            max_resp
        };

        let mut used_command = false;

        for i in 0..max_resp {
            // Interrupt checks
            if self.self_prompter.should_interrupt(self_prompt).await { break; }
            if *self.shut_up.lock().await { break; }
            if self.convo_manager.response_scheduled_for(source).await { break; }

            // Build context for prompt substitution
            let ctx = self.build_prompt_context().await;
            let history = self.history.lock().await.get_history();
            drop(history); // release lock before await

            let history = self.history.lock().await.get_history();
            let response = self.prompter.prompt_convo(&history, &ctx).await?;

            if response.trim().is_empty() {
                warn!("No response from LLM");
                break;
            }

            let cmd_name = contains_command(&response);
            if let Some(ref cmd) = cmd_name {
                let response = trunc_command_message(&response).to_string();
                self.history.lock().await.add(&self.name, &response).await;

                self.self_prompter.handle_user_prompted_cmd(self_prompt, false).await;
                self.route_response(source, &response).await;

                // Execute command
                let exec_result = self.execute_command(&response).await;
                used_command = true;

                if let Some(output) = exec_result {
                    if !output.is_empty() {
                        self.history.lock().await.add("system", &output).await;
                    }
                } else {
                    break;
                }
            } else {
                // Pure conversational response
                self.history.lock().await.add(&self.name, &response).await;
                self.route_response(source, &response).await;
                break;
            }

            self.save_history().await;
        }

        Ok(used_command)
    }

    async fn handle_user_command(
        self: &Arc<Self>,
        source: &str,
        message: &str,
        cmd_name: &str,
    ) -> Result<bool> {
        self.route_response(source, &format!("*{source} used {}*", &cmd_name[1..])).await;
        let result = self.execute_command(message).await;
        if let Some(output) = result {
            self.route_response(source, &output).await;
        }
        Ok(true)
    }

    async fn execute_command(&self, message: &str) -> Option<String> {
        // Delegate to bot's command registry in the minecraft crate
        match self.bot.execute_command(message).await {
            Ok(output) => output,
            Err(e) => Some(format!("Command error: {e}")),
        }
    }

    pub async fn route_response(&self, to: &str, message: &str) {
        if *self.shut_up.lock().await { return; }

        let self_prompt = to == "system" || to == self.name;
        let effective_to = if self_prompt {
            self.last_sender.lock().await.clone().unwrap_or_else(|| to.to_string())
        } else {
            to.to_string()
        };

        if self.convo_manager.is_other_agent(&effective_to).await
            && self.convo_manager.in_conversation(&effective_to).await
        {
            // Send via agent IPC
            let pkg = self.convo_manager.send_to_bot(&effective_to, message.to_string()).await;
            if let Some(p) = pkg {
                self.bot.send_to_agent(&effective_to, &p.message).await;
            }
        } else {
            self.open_chat(message).await;
        }
    }

    pub async fn open_chat(&self, message: &str) {
        let clean = message.replace('\n', " ");
        if self.settings.chat_ingame {
            self.bot.chat(&clean).await;
        }
        // Also send to MindServer (handled by bot layer → server channel)
        self.bot.send_output_to_server(&self.name, &clean).await;
    }

    async fn build_prompt_context(&self) -> PromptContext {
        let memory = self.history.lock().await.memory.clone();
        let sp = if !self.self_prompter.is_stopped().await {
            Some(self.self_prompter.current_prompt().await)
        } else {
            None
        };
        let current_action = Some(self.actions.current_label().await);
        let command_docs = Some(self.bot.get_command_docs(&self.blocked_actions).await);

        PromptContext {
            name: self.name.clone(),
            memory: Some(memory),
            self_prompt: sp,
            current_action,
            command_docs,
            stats: Some(self.bot.get_stats().await),
            inventory: Some(self.bot.get_inventory().await),
            ..Default::default()
        }
    }

    async fn save_history(&self) {
        let sp_state = *self.self_prompter.state.lock().await as u8;
        let sp_prompt = if !self.self_prompter.is_stopped().await {
            Some(self.self_prompter.current_prompt().await)
        } else {
            None
        };
        let last_sender = self.last_sender.lock().await.clone();
        let hist = self.history.lock().await;
        let _ = hist.save(sp_state, sp_prompt.as_deref(), None, last_sender.as_deref());
    }

    pub async fn shut_up(&self) {
        *self.shut_up.lock().await = true;
        self.self_prompter.stop(false).await;
        self.convo_manager.end_all_conversations().await;
    }

    pub async fn clean_kill(&self, msg: &str, code: i32) {
        self.history.lock().await.add("system", msg).await;
        self.bot.chat(if code > 1 { "Restarting." } else { "Exiting." }).await;
        self.save_history().await;
        std::process::exit(code);
    }
}
