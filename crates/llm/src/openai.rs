use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;

use mindcraft_config::{Keys, ModelConfig};

use crate::{LlmProvider, Role, Turn, TurnContent, strict_format};

/// OpenAI-compatible provider. Covers: openai, deepseek, groq, cerebras, xai,
/// mistral, qwen, novita, openrouter, hyperbolic, glhf, vllm, lmstudio.
pub struct OpenAiProvider {
    client: Client,
    model: String,
    base_url: String,
    api_key: String,
    params: HashMap<String, Value>,
}

struct ApiDefaults {
    base_url: &'static str,
    key_name: &'static str,
    default_model: Option<&'static str>,
}

fn api_defaults(api: &str) -> ApiDefaults {
    match api {
        "openai" => ApiDefaults {
            base_url: "https://api.openai.com/v1",
            key_name: "OPENAI_API_KEY",
            default_model: Some("gpt-4o"),
        },
        "deepseek" => ApiDefaults {
            base_url: "https://api.deepseek.com",
            key_name: "DEEPSEEK_API_KEY",
            default_model: Some("deepseek-chat"),
        },
        "groq" => ApiDefaults {
            base_url: "https://api.groq.com/openai/v1",
            key_name: "GROQ_API_KEY",
            default_model: None,
        },
        "cerebras" => ApiDefaults {
            base_url: "https://api.cerebras.ai/v1",
            key_name: "CEREBRAS_API_KEY",
            default_model: None,
        },
        "xai" => ApiDefaults {
            base_url: "https://api.x.ai/v1",
            key_name: "XAI_API_KEY",
            default_model: Some("grok-beta"),
        },
        "mistral" => ApiDefaults {
            base_url: "https://api.mistral.ai/v1",
            key_name: "MISTRAL_API_KEY",
            default_model: Some("mistral-large-latest"),
        },
        "qwen" => ApiDefaults {
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
            key_name: "QWEN_API_KEY",
            default_model: None,
        },
        "novita" => ApiDefaults {
            base_url: "https://api.novita.ai/v3/openai",
            key_name: "NOVITA_API_KEY",
            default_model: None,
        },
        "openrouter" => ApiDefaults {
            base_url: "https://openrouter.ai/api/v1",
            key_name: "OPENROUTER_API_KEY",
            default_model: None,
        },
        "hyperbolic" => ApiDefaults {
            base_url: "https://api.hyperbolic.xyz/v1",
            key_name: "HYPERBOLIC_API_KEY",
            default_model: None,
        },
        "glhf" => ApiDefaults {
            base_url: "https://glhf.chat/api/openai/v1",
            key_name: "GLHF_API_KEY",
            default_model: None,
        },
        "vllm" => ApiDefaults {
            base_url: "http://localhost:8000/v1",
            key_name: "VLLM_API_KEY",
            default_model: None,
        },
        "lmstudio" => ApiDefaults {
            base_url: "http://localhost:1234/v1",
            key_name: "LMSTUDIO_API_KEY",
            default_model: None,
        },
        _ => ApiDefaults {
            base_url: "https://api.openai.com/v1",
            key_name: "OPENAI_API_KEY",
            default_model: None,
        },
    }
}

impl OpenAiProvider {
    pub fn new(cfg: &ModelConfig, keys: &Keys, api: &str) -> Result<Self> {
        let defaults = api_defaults(api);
        let api_key = keys
            .require(defaults.key_name)
            .unwrap_or_else(|_| "no-key".to_string());
        let model = cfg.model.clone()
            .or_else(|| defaults.default_model.map(str::to_string))
            .unwrap_or_else(|| "gpt-4o".to_string());
        let base_url = cfg.url.clone()
            .unwrap_or_else(|| defaults.base_url.to_string());
        Ok(Self {
            client: Client::new(),
            model,
            base_url,
            api_key,
            params: cfg.params.clone(),
        })
    }

    fn build_messages(&self, turns: &[Turn], system: &str) -> Vec<Value> {
        let mut msgs = vec![json!({ "role": "system", "content": system })];
        for t in strict_format(turns) {
            let role = match t.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let content: Value = match t.content {
                TurnContent::Text(s) => Value::String(s),
                TurnContent::Parts(parts) => {
                    Value::Array(parts.into_iter().map(|p| match p {
                        crate::ContentPart::Text { text } => {
                            json!({ "type": "text", "text": text })
                        }
                        crate::ContentPart::Image { url } => {
                            json!({ "type": "image_url", "image_url": { "url": url } })
                        }
                        crate::ContentPart::ImageData { media_type, data } => {
                            json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{media_type};base64,{data}")
                                }
                            })
                        }
                    }).collect())
                }
            };
            msgs.push(json!({ "role": role, "content": content }));
        }
        msgs
    }

    async fn chat_complete(&self, messages: Vec<Value>) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut body = json!({
            "model": self.model,
            "messages": messages,
        });
        if let Some(obj) = body.as_object_mut() {
            for (k, v) in &self.params {
                obj.insert(k.clone(), v.clone());
            }
        }

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("sending OpenAI-compat request")?;

        let status = resp.status();
        let text = resp.text().await.context("reading OpenAI response")?;
        if !status.is_success() {
            anyhow::bail!("OpenAI API {status}: {text}");
        }

        let parsed: Value = serde_json::from_str(&text)?;
        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no content in OpenAI response: {text}"))?
            .to_string();

        Ok(content)
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn send_request(&self, turns: &[Turn], system: &str) -> Result<String> {
        let messages = self.build_messages(turns, system);
        let mut result = self.chat_complete(messages).await?;
        if let Some(pos) = result.find("</think>") {
            result = result[pos + "</think>".len()..].to_string();
        }
        Ok(result)
    }

    async fn send_vision_request(&self, turns: &[Turn], system: &str, image: &[u8]) -> Result<String> {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, image);
        let mut vision_turns = turns.to_vec();
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

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = json!({
            "model": "text-embedding-3-small",
            "input": text,
        });
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let parsed: Value = resp.json().await?;
        let embedding = parsed["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no embedding in response"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        Ok(embedding)
    }
}
