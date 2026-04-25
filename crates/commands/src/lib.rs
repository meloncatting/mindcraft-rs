//! Command parsing and dispatch.
//! Mirrors src/agent/commands/index.js + actions.js + queries.js.

pub mod parser;
pub mod registry;

pub use parser::{parse_command, ParsedCommand};
pub use registry::{CommandRegistry, CommandDef, CommandParam, ParamType};

use anyhow::Result;
use async_trait::async_trait;

/// Context passed to every command's perform function.
/// Holds a weakened reference to the agent via a trait object so we avoid
/// circular dependencies between crates.
#[async_trait]
pub trait AgentContext: Send + Sync {
    fn name(&self) -> &str;
    fn is_insecure_coding_allowed(&self) -> bool;
    async fn get_stats(&self) -> String;
    async fn get_inventory(&self) -> String;
    async fn get_nearby_entities(&self) -> String;
    async fn get_nearby_blocks(&self) -> String;
    async fn stop_actions(&self);
    async fn cancel_resume(&self);
    async fn emit_idle(&self);
    fn behavior_log_mut(&self) -> String;
    fn add_to_history(&self, role: &str, content: &str);
    fn set_self_prompt(&self, prompt: Option<String>);
    fn get_self_prompt(&self) -> Option<String>;
    fn set_mode(&self, mode: &str, enabled: bool);
    fn get_mode(&self, mode: &str) -> bool;
    fn recall_place(&self, name: &str) -> Option<[f64; 3]>;
    fn remember_place(&self, name: &str, pos: [f64; 3]);
    fn get_place_names(&self) -> String;
}

/// Command result
pub type CommandResult = Result<Option<String>>;

/// All commands implement this.
#[async_trait]
pub trait Command: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn params(&self) -> &[CommandParam];
    fn is_action(&self) -> bool { false }
    async fn perform(&self, ctx: &dyn AgentContext, args: &[CommandArg]) -> CommandResult;
}

#[derive(Debug, Clone)]
pub enum CommandArg {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
}

impl CommandArg {
    pub fn as_str(&self) -> &str {
        match self {
            CommandArg::Text(s) => s,
            _ => "",
        }
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            CommandArg::Float(f) => *f,
            CommandArg::Int(i) => *i as f64,
            _ => 0.0,
        }
    }
    pub fn as_i64(&self) -> i64 {
        match self {
            CommandArg::Int(i) => *i,
            CommandArg::Float(f) => *f as i64,
            _ => 0,
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            CommandArg::Bool(b) => *b,
            _ => false,
        }
    }
}
