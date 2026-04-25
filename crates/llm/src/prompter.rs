//! Prompter: profile-driven prompt construction + LLM dispatch.
//! Mirrors src/models/prompter.js.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

use mindcraft_config::{resolve_model_spec, AgentProfile, Settings};

use crate::{LlmProvider, Turn, Role, create_provider};
use mindcraft_config::Keys;

/// Placeholder tokens used in profile prompt templates.
const PLACEHOLDERS: &[&str] = &[
    "$NAME", "$STATS", "$INVENTORY", "$ACTION", "$COMMAND_DOCS",
    "$CODE_DOCS", "$EXAMPLES", "$MEMORY", "$TO_SUMMARIZE",
    "$CONVO", "$SELF_PROMPT", "$LAST_GOALS", "$BLUEPRINTS",
];

pub struct Prompter {
    pub profile: AgentProfile,

    /// Primary chat/reasoning model
    pub chat_model: Arc<dyn LlmProvider>,
    /// Code generation model (defaults to chat_model)
    pub code_model: Arc<dyn LlmProvider>,
    /// Vision model (defaults to chat_model)
    pub vision_model: Arc<dyn LlmProvider>,
    /// Embedding model (defaults to chat_model or a dedicated embedder)
    pub embedding_model: Arc<dyn LlmProvider>,

    /// Cooldown between prompts (ms)
    cooldown_ms: u64,
    last_prompt_time: Mutex<std::time::Instant>,

    /// Prevents concurrent coding prompts
    awaiting_coding: Mutex<bool>,
    /// Most recent message timestamp for staleness detection
    most_recent_msg_time: Mutex<std::time::Instant>,
}

impl Prompter {
    pub fn new(profile: AgentProfile, settings: &Settings, keys: &Keys) -> Result<Arc<Self>> {
        let build = |spec: &mindcraft_config::ModelSpec| -> Result<Arc<dyn LlmProvider>> {
            let cfg = resolve_model_spec(spec);
            Ok(Arc::from(create_provider(&cfg, keys)?))
        };

        let chat_model = build(&profile.model)?;
        let code_model = profile.code_model.as_ref()
            .map(|s| build(s))
            .transpose()?
            .unwrap_or_else(|| chat_model.clone());
        let vision_model = profile.vision_model.as_ref()
            .map(|s| build(s))
            .transpose()?
            .unwrap_or_else(|| chat_model.clone());
        let embedding_model = profile.embedding.as_ref()
            .map(|s| build(s))
            .transpose()?
            .unwrap_or_else(|| chat_model.clone());

        let cooldown_ms = profile.cooldown;

        Ok(Arc::new(Self {
            profile,
            chat_model,
            code_model,
            vision_model,
            embedding_model,
            cooldown_ms,
            last_prompt_time: Mutex::new(
                std::time::Instant::now() - std::time::Duration::from_secs(9999),
            ),
            awaiting_coding: Mutex::new(false),
            most_recent_msg_time: Mutex::new(std::time::Instant::now()),
        }))
    }

    pub fn name(&self) -> &str {
        &self.profile.name
    }

    async fn wait_cooldown(&self) {
        if self.cooldown_ms == 0 { return; }
        let elapsed = {
            let t = self.last_prompt_time.lock().await;
            t.elapsed().as_millis() as u64
        };
        if elapsed < self.cooldown_ms {
            tokio::time::sleep(std::time::Duration::from_millis(
                self.cooldown_ms - elapsed,
            )).await;
        }
        *self.last_prompt_time.lock().await = std::time::Instant::now();
    }

    /// Replace $PLACEHOLDER tokens in a prompt template.
    /// Context struct carries pre-computed values for each placeholder.
    pub async fn replace_strings(&self, mut prompt: String, ctx: &PromptContext) -> String {
        prompt = prompt.replace("$NAME", &ctx.name);
        if let Some(stats) = &ctx.stats { prompt = prompt.replace("$STATS", stats); }
        if let Some(inv) = &ctx.inventory { prompt = prompt.replace("$INVENTORY", inv); }
        if let Some(action) = &ctx.current_action { prompt = prompt.replace("$ACTION", action); }
        if let Some(docs) = &ctx.command_docs { prompt = prompt.replace("$COMMAND_DOCS", docs); }
        if let Some(code_docs) = &ctx.code_docs { prompt = prompt.replace("$CODE_DOCS", code_docs); }
        if let Some(examples) = &ctx.examples { prompt = prompt.replace("$EXAMPLES", examples); }
        if let Some(memory) = &ctx.memory { prompt = prompt.replace("$MEMORY", memory); }
        if let Some(to_sum) = &ctx.to_summarize { prompt = prompt.replace("$TO_SUMMARIZE", to_sum); }
        if let Some(convo) = &ctx.convo { prompt = prompt.replace("$CONVO", &format!("Recent conversation:\n{convo}")); }
        if let Some(sp) = &ctx.self_prompt {
            prompt = prompt.replace("$SELF_PROMPT", &format!("YOUR CURRENT ASSIGNED GOAL: \"{sp}\"\n"));
        } else {
            prompt = prompt.replace("$SELF_PROMPT", "");
        }
        if let Some(goals) = &ctx.last_goals {
            let goal_text = goals.iter().map(|(g, ok)| {
                if *ok { format!("You recently successfully completed the goal {g}.\n") }
                else { format!("You recently failed to complete the goal {g}.\n") }
            }).collect::<String>();
            prompt = prompt.replace("$LAST_GOALS", goal_text.trim());
        }
        if let Some(bp) = &ctx.blueprints { prompt = prompt.replace("$BLUEPRINTS", bp); }

        // Warn on unknown placeholders
        let re = regex::Regex::new(r"\$[A-Z_]+").unwrap();
        for m in re.find_iter(&prompt) {
            warn!("Unknown prompt placeholder: {}", m.as_str());
        }
        prompt
    }

    /// Main conversation prompt. Returns empty string if response is stale.
    pub async fn prompt_convo(&self, turns: &[Turn], ctx: &PromptContext) -> Result<String> {
        let msg_time = std::time::Instant::now();
        *self.most_recent_msg_time.lock().await = msg_time;

        // Up to 3 retries to avoid hallucinations
        for attempt in 0..3 {
            self.wait_cooldown().await;

            // Stale check: a newer message arrived while we waited
            if *self.most_recent_msg_time.lock().await != msg_time {
                return Ok(String::new());
            }

            let prompt = self.replace_strings(self.profile.conversing.clone(), ctx).await;
            match self.chat_model.send_request(turns, &prompt).await {
                Ok(resp) => {
                    if resp.contains("(FROM OTHER BOT)") {
                        warn!("LLM hallucinated other-bot message, attempt {attempt}");
                        continue;
                    }
                    if *self.most_recent_msg_time.lock().await != msg_time {
                        warn!("Discarding stale response");
                        return Ok(String::new());
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    warn!("LLM error on attempt {attempt}: {e}");
                    continue;
                }
            }
        }
        Ok(String::new())
    }

    pub async fn prompt_coding(&self, turns: &[Turn], ctx: &PromptContext) -> Result<String> {
        {
            let mut flag = self.awaiting_coding.lock().await;
            if *flag {
                warn!("Already awaiting coding response");
                return Ok("```//no response```".to_string());
            }
            *flag = true;
        }
        self.wait_cooldown().await;
        let prompt = self.replace_strings(self.profile.coding.clone(), ctx).await;
        let resp = self.code_model.send_request(turns, &prompt).await;
        *self.awaiting_coding.lock().await = false;
        resp
    }

    pub async fn prompt_mem_saving(&self, to_summarize: &str) -> Result<String> {
        self.wait_cooldown().await;
        let ctx = PromptContext {
            name: self.profile.name.clone(),
            to_summarize: Some(to_summarize.to_string()),
            ..Default::default()
        };
        let prompt = self.replace_strings(self.profile.saving_memory.clone(), &ctx).await;
        let mut resp = self.chat_model.send_request(&[], &prompt).await?;
        if let Some(pos) = resp.find("</think>") {
            resp = resp[pos + "</think>".len()..].to_string();
        }
        Ok(resp)
    }

    pub async fn prompt_should_respond_to_bot(
        &self,
        turns: &[Turn],
        new_message: &str,
    ) -> Result<bool> {
        self.wait_cooldown().await;
        let mut all_turns = turns.to_vec();
        all_turns.push(Turn::user(new_message));
        let ctx = PromptContext {
            name: self.profile.name.clone(),
            convo: Some(stringify_turns(&all_turns)),
            ..Default::default()
        };
        let prompt = self.replace_strings(self.profile.bot_responder.clone(), &ctx).await;
        let resp = self.chat_model.send_request(&[], &prompt).await?;
        Ok(resp.trim().to_lowercase() == "respond")
    }

    pub async fn prompt_vision(
        &self,
        turns: &[Turn],
        system: &str,
        image: &[u8],
    ) -> Result<String> {
        self.wait_cooldown().await;
        self.vision_model.send_vision_request(turns, system, image).await
    }
}

/// Pre-computed context values for placeholder substitution.
/// Callers (Agent) populate this before calling prompter methods.
#[derive(Debug, Default, Clone)]
pub struct PromptContext {
    pub name: String,
    pub stats: Option<String>,
    pub inventory: Option<String>,
    pub current_action: Option<String>,
    pub command_docs: Option<String>,
    pub code_docs: Option<String>,
    pub examples: Option<String>,
    pub memory: Option<String>,
    pub to_summarize: Option<String>,
    pub convo: Option<String>,
    pub self_prompt: Option<String>,
    pub last_goals: Option<Vec<(String, bool)>>,
    pub blueprints: Option<String>,
}

pub fn stringify_turns(turns: &[Turn]) -> String {
    turns.iter().map(|t| {
        let role = match t.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        format!("{}: {}\n", role, t.content.text())
    }).collect()
}
