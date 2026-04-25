use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use mindcraft_config::ModelConfig;
use crate::{LlmProvider, Turn, openai::OpenAiProvider};

const DEFAULT_URL: &str = "http://localhost:11434";

/// Ollama provider uses the OpenAI-compatible /v1 endpoint.
pub struct OllamaProvider {
    inner: OpenAiProvider,
}

impl OllamaProvider {
    pub fn new(cfg: &ModelConfig) -> Result<Self> {
        let url = cfg.url.clone().unwrap_or_else(|| DEFAULT_URL.to_string());
        let mut patched = cfg.clone();
        patched.url = Some(format!("{url}/v1"));
        // Ollama doesn't need a real key
        use mindcraft_config::Keys;
        let keys = Keys::default();
        Ok(Self {
            inner: OpenAiProvider::new(&patched, &keys, "ollama")?,
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn send_request(&self, turns: &[Turn], system: &str) -> Result<String> {
        self.inner.send_request(turns, system).await
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Ollama has a native /api/embeddings endpoint
        let client = Client::new();
        let model = "nomic-embed-text"; // sensible default
        let body = json!({ "model": model, "prompt": text });
        let resp = client
            .post(format!("{DEFAULT_URL}/api/embeddings"))
            .json(&body)
            .send()
            .await
            .context("sending Ollama embed request")?;
        let parsed: Value = resp.json().await?;
        let embedding = parsed["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no embedding in Ollama response"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        Ok(embedding)
    }
}
