pub mod anthropic;
pub mod openai;
pub mod gemini;
pub mod ollama;
pub mod prompter;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use mindcraft_config::{ModelConfig, Keys};

/// Unified conversation turn (maps to OpenAI message format used everywhere)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: Role,
    pub content: TurnContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TurnContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl TurnContent {
    pub fn text(&self) -> &str {
        match self {
            TurnContent::Text(s) => s,
            TurnContent::Parts(parts) => {
                parts.iter().find_map(|p| {
                    if let ContentPart::Text { text } = p { Some(text.as_str()) } else { None }
                }).unwrap_or("")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContentPart {
    Text { text: String },
    Image { url: String },
    ImageData { media_type: String, data: String },
}

impl Turn {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: TurnContent::Text(content.into()) }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: TurnContent::Text(content.into()) }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: TurnContent::Text(content.into()) }
    }
}

/// Core LLM provider trait. All providers implement this.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request. `system` is injected as a system message.
    async fn send_request(&self, turns: &[Turn], system: &str) -> Result<String>;

    /// Vision request with an image buffer. Default impl: unsupported.
    async fn send_vision_request(
        &self,
        turns: &[Turn],
        system: &str,
        image: &[u8],
    ) -> Result<String> {
        let _ = (turns, system, image);
        anyhow::bail!("vision not supported by this provider")
    }

    /// Embedding request. Default impl: unsupported.
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let _ = text;
        anyhow::bail!("embeddings not supported by this provider")
    }
}

/// Factory: construct a boxed LlmProvider from a resolved ModelConfig + key store.
pub fn create_provider(cfg: &ModelConfig, keys: &Keys) -> Result<Box<dyn LlmProvider>> {
    let api = cfg.api.as_deref().unwrap_or("openai");
    match api {
        "anthropic" => Ok(Box::new(anthropic::ClaudeProvider::new(cfg, keys)?)),
        "openai" | "deepseek" | "groq" | "cerebras" | "xai" |
        "mistral" | "qwen" | "novita" | "openrouter" | "hyperbolic" |
        "glhf" | "vllm" | "lmstudio" => {
            Ok(Box::new(openai::OpenAiProvider::new(cfg, keys, api)?))
        }
        "google" => Ok(Box::new(gemini::GeminiProvider::new(cfg, keys)?)),
        "ollama" => Ok(Box::new(ollama::OllamaProvider::new(cfg)?)),
        other => anyhow::bail!("unknown API provider: {other}"),
    }
}

/// Normalize turns so they alternate user/assistant (some APIs require this).
/// Consecutive same-role messages are merged. System messages become a system turn.
pub fn strict_format(turns: &[Turn]) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    for turn in turns {
        if turn.role == Role::System {
            // Prepend system content to next user message
            out.push(Turn::user(format!("[system]: {}", turn.content.text())));
            continue;
        }
        if let Some(last) = out.last_mut() {
            if last.role == turn.role {
                // Merge into existing turn
                if let (TurnContent::Text(a), TurnContent::Text(b)) =
                    (&mut last.content, &turn.content)
                {
                    *a = format!("{}\n{}", a, b);
                    continue;
                }
            }
        }
        out.push(turn.clone());
    }
    // API requires alternating, starting with user
    if out.first().map(|t| &t.role) == Some(&Role::Assistant) {
        out.insert(0, Turn::user(""));
    }
    out
}
