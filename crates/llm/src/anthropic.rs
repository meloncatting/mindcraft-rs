use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use mindcraft_config::{Keys, ModelConfig};

use crate::{LlmProvider, Role, Turn, TurnContent, strict_format};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct ClaudeProvider {
    client: Client,
    model: String,
    base_url: String,
    api_key: String,
    params: HashMap<String, Value>,
}

impl ClaudeProvider {
    pub fn new(cfg: &ModelConfig, keys: &Keys) -> Result<Self> {
        let api_key = keys.require("ANTHROPIC_API_KEY")?;
        let model = cfg.model.clone().unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let base_url = cfg.url.clone().unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Ok(Self {
            client: Client::new(),
            model,
            base_url,
            api_key,
            params: cfg.params.clone(),
        })
    }

    fn max_tokens(&self) -> u32 {
        if let Some(mt) = self.params.get("max_tokens").and_then(|v| v.as_u64()) {
            return mt as u32;
        }
        if let Some(budget) = self.params
            .get("thinking")
            .and_then(|v| v.get("budget_tokens"))
            .and_then(|v| v.as_u64())
        {
            return (budget + 1000) as u32;
        }
        4096
    }

    fn build_messages(&self, turns: &[Turn]) -> Vec<Value> {
        strict_format(turns)
            .into_iter()
            .filter(|t| t.role != Role::System)
            .map(|t| {
                let role = match t.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "user",
                };
                match t.content {
                    TurnContent::Text(s) => json!({ "role": role, "content": s }),
                    TurnContent::Parts(parts) => {
                        let content: Vec<Value> = parts
                            .into_iter()
                            .map(|p| match p {
                                crate::ContentPart::Text { text } => {
                                    json!({ "type": "text", "text": text })
                                }
                                crate::ContentPart::ImageData { media_type, data } => {
                                    json!({
                                        "type": "image",
                                        "source": {
                                            "type": "base64",
                                            "media_type": media_type,
                                            "data": data
                                        }
                                    })
                                }
                                crate::ContentPart::Image { url } => {
                                    json!({
                                        "type": "image",
                                        "source": { "type": "url", "url": url }
                                    })
                                }
                            })
                            .collect();
                        json!({ "role": role, "content": content })
                    }
                }
            })
            .collect()
    }

    async fn request(&self, body: Value) -> Result<String> {
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("sending Anthropic request")?;

        let status = resp.status();
        let text = resp.text().await.context("reading Anthropic response body")?;

        if !status.is_success() {
            anyhow::bail!("Anthropic API error {status}: {text}");
        }

        let parsed: Value = serde_json::from_str(&text).context("parsing Anthropic response")?;

        // Extract first text content block
        parsed["content"]
            .as_array()
            .and_then(|arr| {
                arr.iter().find(|item| item["type"] == "text")
                    .and_then(|item| item["text"].as_str())
                    .map(str::to_string)
            })
            .ok_or_else(|| anyhow::anyhow!("no text content in Anthropic response"))
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    async fn send_request(&self, turns: &[Turn], system: &str) -> Result<String> {
        let messages = self.build_messages(turns);
        let mut body = json!({
            "model": self.model,
            "system": system,
            "messages": messages,
            "max_tokens": self.max_tokens(),
        });

        // Merge extra params (thinking, temperature, etc.)
        if let Some(obj) = body.as_object_mut() {
            for (k, v) in &self.params {
                if k != "max_tokens" {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        let mut result = self.request(body).await?;

        // Strip <think> blocks (reasoning models)
        if let Some(pos) = result.find("</think>") {
            result = result[pos + "</think>".len()..].to_string();
        }

        Ok(result)
    }

    async fn send_vision_request(&self, turns: &[Turn], system: &str, image: &[u8]) -> Result<String> {
        let mut vision_turns = turns.to_vec();
        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            image,
        );
        vision_turns.push(Turn {
            role: Role::User,
            content: TurnContent::Parts(vec![
                crate::ContentPart::Text { text: system.to_string() },
                crate::ContentPart::ImageData {
                    media_type: "image/jpeg".to_string(),
                    data: b64,
                },
            ]),
        });
        self.send_request(&vision_turns, system).await
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        anyhow::bail!("Anthropic does not support embeddings")
    }
}
