use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use anyhow::{Context, Result};

/// Top-level runtime settings (mirrors settings.js)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub minecraft_version: String,
    pub host: String,
    pub port: i32,
    pub auth: AuthMode,

    pub mindserver_port: u16,
    pub auto_open_ui: bool,

    pub base_profile: BaseProfile,
    pub profiles: Vec<String>,

    pub load_memory: bool,
    pub init_message: Option<String>,
    pub only_chat_with: Vec<String>,

    pub speak: bool,
    pub chat_ingame: bool,
    pub language: String,
    pub render_bot_view: bool,

    pub allow_insecure_coding: bool,
    pub allow_vision: bool,
    pub blocked_actions: Vec<String>,
    pub code_timeout_mins: i32,
    pub relevant_docs_count: i32,

    pub max_messages: usize,
    pub num_examples: usize,
    pub max_commands: i32,
    pub show_command_syntax: CommandSyntaxMode,
    pub narrate_behavior: bool,
    pub chat_bot_messages: bool,

    pub spawn_timeout: u64,
    pub block_place_delay: u64,

    pub log_all_prompts: bool,

    /// Injected after loading individual agent profile
    #[serde(skip)]
    pub profile: Option<AgentProfile>,

    /// Injected after loading task file
    #[serde(skip)]
    pub task: Option<TaskConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minecraft_version: "auto".into(),
            host: "127.0.0.1".into(),
            port: 55916,
            auth: AuthMode::Offline,
            mindserver_port: 8080,
            auto_open_ui: true,
            base_profile: BaseProfile::Assistant,
            profiles: vec!["./andy.json".into()],
            load_memory: false,
            init_message: None,
            only_chat_with: vec![],
            speak: false,
            chat_ingame: true,
            language: "en".into(),
            render_bot_view: false,
            allow_insecure_coding: false,
            allow_vision: false,
            blocked_actions: vec![
                "!checkBlueprint".into(),
                "!checkBlueprintLevel".into(),
                "!getBlueprint".into(),
                "!getBlueprintLevel".into(),
            ],
            code_timeout_mins: -1,
            relevant_docs_count: 5,
            max_messages: 15,
            num_examples: 2,
            max_commands: -1,
            show_command_syntax: CommandSyntaxMode::Full,
            narrate_behavior: true,
            chat_bot_messages: true,
            spawn_timeout: 30,
            block_place_delay: 0,
            log_all_prompts: false,
            profile: None,
            task: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    Offline,
    Microsoft,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaseProfile {
    Survival,
    Assistant,
    Creative,
    GodMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommandSyntaxMode {
    Full,
    Shortened,
    None,
}

/// Per-agent profile (andy.json / claude.json / etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,

    /// Model spec: "gpt-4o", "anthropic/claude-opus-4-7", {api:"anthropic",model:"..."}, etc.
    pub model: ModelSpec,

    #[serde(default)]
    pub code_model: Option<ModelSpec>,
    #[serde(default)]
    pub vision_model: Option<ModelSpec>,
    #[serde(default)]
    pub embedding: Option<ModelSpec>,

    /// System prompt templates. These use $PLACEHOLDER substitution.
    #[serde(default)]
    pub conversing: String,
    #[serde(default)]
    pub coding: String,
    #[serde(default)]
    pub saving_memory: String,
    #[serde(default)]
    pub bot_responder: String,
    #[serde(default)]
    pub image_analysis: String,

    #[serde(default)]
    pub conversation_examples: Vec<serde_json::Value>,
    #[serde(default)]
    pub coding_examples: Vec<serde_json::Value>,

    #[serde(default)]
    pub modes: HashMap<String, bool>,

    #[serde(default)]
    pub cooldown: u64,

    #[serde(default)]
    pub skin: Option<SkinConfig>,
    #[serde(default)]
    pub speak_model: Option<String>,

    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelSpec {
    /// Simple string like "gpt-4o" or "anthropic/claude-opus-4-7"
    String(String),
    /// Detailed spec: { api: "anthropic", model: "claude-opus-4-7", url: "...", params: {...} }
    Object(ModelConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub api: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinConfig {
    pub model: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskConfig {
    pub task_id: Option<String>,
    pub agent_names: Option<Vec<String>>,
    pub blocked_actions: Option<Vec<String>>,
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}

/// Resolve a ModelSpec to a concrete (api, model, url, params) tuple.
/// Mirrors selectAPI() in _model_map.js.
pub fn resolve_model_spec(spec: &ModelSpec) -> ModelConfig {
    match spec {
        ModelSpec::Object(cfg) => cfg.clone(),
        ModelSpec::String(s) => {
            let (api, model) = infer_api_and_model(s);
            ModelConfig {
                api: Some(api),
                model: Some(model),
                url: None,
                params: HashMap::new(),
            }
        }
    }
}

fn infer_api_and_model(s: &str) -> (String, String) {
    // "anthropic/claude-opus-4-7" → ("anthropic", "claude-opus-4-7")
    let known_prefixes = [
        "openai", "anthropic", "google", "xai", "mistral",
        "deepseek", "qwen", "ollama", "groq", "cerebras",
        "vllm", "lmstudio", "replicate", "novita", "openrouter",
        "hyperbolic", "huggingface", "glhf",
    ];
    for prefix in known_prefixes {
        if let Some(rest) = s.strip_prefix(&format!("{}/", prefix)) {
            return (prefix.to_string(), rest.to_string());
        }
    }
    // No prefix: infer from model name heuristics
    let api = if s.contains("gpt") || s.contains("o1") || s.contains("o3") {
        "openai"
    } else if s.contains("claude") {
        "anthropic"
    } else if s.contains("gemini") {
        "google"
    } else if s.contains("grok") {
        "xai"
    } else if s.contains("mistral") {
        "mistral"
    } else if s.contains("deepseek") {
        "deepseek"
    } else if s.contains("qwen") {
        "qwen"
    } else {
        "openai" // fallback
    };
    (api.to_string(), s.to_string())
}

impl Settings {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading settings from {:?}", path.as_ref()))?;
        let settings: Self = serde_json::from_str(&content)
            .with_context(|| "parsing settings JSON")?;
        Ok(settings)
    }

    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        Self::load(path).unwrap_or_default()
    }
}

/// Load and merge agent profile with base profile and defaults.
/// Mirrors the merging logic in Prompter constructor.
pub fn load_profile(profile_path: &str, base_profile: &BaseProfile) -> Result<AgentProfile> {
    let profile_str = std::fs::read_to_string(profile_path)
        .with_context(|| format!("reading profile {profile_path}"))?;
    let mut profile: serde_json::Value = serde_json::from_str(&profile_str)?;

    // Load default profile
    let default_path = format!("profiles/defaults/_default.json");
    let base_path = match base_profile {
        BaseProfile::Survival  => "profiles/defaults/survival.json",
        BaseProfile::Assistant => "profiles/defaults/assistant.json",
        BaseProfile::Creative  => "profiles/defaults/creative.json",
        BaseProfile::GodMode   => "profiles/defaults/god_mode.json",
    };

    let default_val: serde_json::Value = load_json_or_empty(&default_path);
    let base_val: serde_json::Value = load_json_or_empty(base_path);

    // Merge: default ← base ← individual (individual wins)
    let merged = merge_profiles(default_val, base_val, profile);

    serde_json::from_value(merged).context("deserializing merged profile")
}

fn load_json_or_empty(path: &str) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Object(Default::default()))
}

fn merge_profiles(
    default: serde_json::Value,
    base: serde_json::Value,
    individual: serde_json::Value,
) -> serde_json::Value {
    let mut result = match default {
        serde_json::Value::Object(m) => m,
        _ => Default::default(),
    };
    if let serde_json::Value::Object(base_map) = base {
        for (k, v) in base_map {
            result.insert(k, v);
        }
    }
    if let serde_json::Value::Object(ind_map) = individual {
        for (k, v) in ind_map {
            result.insert(k, v);
        }
    }
    serde_json::Value::Object(result)
}

/// API keys store. Mirrors keys.js.
#[derive(Debug, Clone, Default)]
pub struct Keys {
    inner: HashMap<String, String>,
}

impl Keys {
    pub fn load(path: &str) -> Self {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let map: HashMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
        Self { inner: map }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.inner.get(key).map(String::as_str)
            .or_else(|| std::env::var(key).ok().map(|_| "").and(None))
    }

    pub fn require(&self, key: &str) -> Result<String> {
        self.inner.get(key).cloned()
            .or_else(|| std::env::var(key).ok())
            .with_context(|| format!("missing API key: {key}"))
    }
}
