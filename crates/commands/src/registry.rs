//! Command registry with typed parameter specs and dispatch.

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{Result, bail};

use crate::{Command, CommandArg, AgentContext, CommandResult};
use crate::parser::{parse_command, coerce_arg};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    Int,
    Float,
    Boolean,
    String,
    BlockName,
    ItemName,
    BlockOrItemName,
}

#[derive(Debug, Clone)]
pub struct CommandParam {
    pub name: String,
    pub param_type: ParamType,
    pub description: String,
    /// Optional [min, max] domain for numeric params
    pub domain: Option<[f64; 2]>,
}

#[derive(Debug, Clone)]
pub struct CommandDef {
    pub name: String,
    pub description: String,
    pub params: Vec<CommandParam>,
    pub is_action: bool,
}

/// Central registry for all commands.
pub struct CommandRegistry {
    commands: HashMap<String, Arc<dyn Command>>,
    unblockable: &'static [&'static str],
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            unblockable: &["!stop", "!stats", "!inventory", "!goal"],
        }
    }

    pub fn register(&mut self, cmd: Arc<dyn Command>) {
        self.commands.insert(cmd.name().to_string(), cmd);
    }

    pub fn blacklist(&mut self, blocked: &[String]) {
        for name in blocked {
            if self.unblockable.contains(&name.as_str()) {
                tracing::warn!("Command {name} is unblockable, ignoring blacklist");
                continue;
            }
            self.commands.remove(name);
        }
    }

    pub fn exists(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Command>> {
        self.commands.get(name)
    }

    pub fn is_action(&self, name: &str) -> bool {
        self.commands.get(name).map(|c| c.is_action()).unwrap_or(false)
    }

    /// Parse and execute a command from a raw message string.
    pub async fn execute(
        &self,
        ctx: &dyn AgentContext,
        message: &str,
    ) -> Result<Option<String>> {
        let parsed = parse_command(message)?;
        let cmd = self.commands.get(&parsed.name)
            .ok_or_else(|| anyhow::anyhow!("Command {} does not exist.", parsed.name))?;

        let params = cmd.params();
        if parsed.raw_args.len() != params.len() {
            bail!(
                "Command {} was given {} args, but requires {}.",
                parsed.name,
                parsed.raw_args.len(),
                params.len()
            );
        }

        let mut typed_args: Vec<CommandArg> = Vec::new();
        for (raw, param) in parsed.raw_args.iter().zip(params.iter()) {
            let arg = coerce_arg(raw, &param.param_type)
                .map_err(|e| anyhow::anyhow!("Param '{}': {e}", param.name))?;

            // Domain check for numerics
            if let Some([lo, hi]) = param.domain {
                let v = match &arg {
                    CommandArg::Int(i) => *i as f64,
                    CommandArg::Float(f) => *f,
                    _ => f64::NAN,
                };
                if !v.is_nan() && (v < lo || v >= hi) {
                    bail!("Param '{}' must be in [{lo}, {hi}).", param.name);
                }
            }
            typed_args.push(arg);
        }

        cmd.perform(ctx, &typed_args).await
    }

    /// Generate command documentation string for LLM prompts.
    pub fn get_docs(&self, blocked: &[String]) -> String {
        let mut docs = "\n*COMMAND DOCS\n You can use the following commands to perform actions \
            and get information about the world. \
            Use the commands with the syntax: !commandName or \
            !commandName(\"arg1\", 1.2, ...) if the command takes arguments.\n\
            Do not use codeblocks. Use double quotes for strings. \
            Only use one command in each response, trailing commands and comments will be ignored.\n"
            .to_string();

        let mut names: Vec<&str> = self.commands.keys().map(String::as_str).collect();
        names.sort();

        for name in names {
            if blocked.contains(&name.to_string()) { continue; }
            let cmd = &self.commands[name];
            docs.push_str(&format!("{}: {}\n", cmd.name(), cmd.description()));
            for param in cmd.params() {
                let type_str = match &param.param_type {
                    ParamType::Float | ParamType::Int => "number",
                    ParamType::BlockName | ParamType::ItemName |
                    ParamType::BlockOrItemName | ParamType::String => "string",
                    ParamType::Boolean => "bool",
                };
                docs.push_str(&format!(
                    "  {}: ({}) {}\n",
                    param.name, type_str, param.description
                ));
            }
        }
        docs.push_str("*\n");
        docs
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
