use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use mindcraft_config::{Keys, ModelConfig};
use crate::{LlmProvider, Role, Turn, TurnContent};

const DEFAULT_MODEL: &str = "gemini-2.0-flash";
const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

pub struct GeminiProvider {
    client: Client,
    model: String,
    api_key: String,
}

impl GeminiProvider {
    pub fn new(cfg: &ModelConfig, keys: &Keys) -> Result<Self> {
        let api_key = keys.require("GEMINI_API_KEY")?;
        let model = cfg.model.clone().unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Ok(Self { client: Client::new(), model, api_key })
    }

    fn turns_to_contents(turns: &[Turn], system: &str) -> (Option<Value>, Vec<Value>) {
        let system_instruction = if system.is_empty() { None } else {
            Some(json!({ "parts": [{ "text": system }] }))
        };
        let contents: Vec<Value> = turns.iter().map(|t| {
            let role = match t.role {
                Role::User | Role::System => "user",
                Role::Assistant => "model",
            };
            let parts: Vec<Value> = match &t.content {
                TurnContent::Text(s) => vec![json!({ "text": s })],
                TurnContent::Parts(parts) => parts.iter().map(|p| match p {
                    crate::ContentPart::Text { text } => json!({ "text": text }),
                    crate::ContentPart::ImageData { media_type, data } => json!({
                        "inlineData": { "mimeType": media_type, "data": data }
                    }),
                    crate::ContentPart::Image { url } => json!({ "text": url }),
                }).collect(),
            };
            json!({ "role": role, "parts": parts })
        }).collect();
        (system_instruction, contents)
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn send_request(&self, turns: &[Turn], system: &str) -> Result<String> {
        let (sys_inst, contents) = Self::turns_to_contents(turns, system);
        let mut body = json!({ "contents": contents });
        if let Some(si) = sys_inst {
            body["systemInstruction"] = si;
        }

        let url = format!("{}/{}:generateContent?key={}", BASE_URL, self.model, self.api_key);
        let resp = self.client.post(&url).json(&body).send().await
            .context("sending Gemini request")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("Gemini API {status}: {text}");
        }

        let parsed: Value = serde_json::from_str(&text)?;
        parsed["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("no text in Gemini response: {text}"))
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/text-embedding-004:embedContent?key={}", BASE_URL, self.api_key);
        let body = json!({ "content": { "parts": [{ "text": text }] } });
        let resp = self.client.post(&url).json(&body).send().await?;
        let parsed: Value = resp.json().await?;
        let embedding = parsed["embedding"]["values"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no embedding values"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        Ok(embedding)
    }
}
