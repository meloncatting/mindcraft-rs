//! Command message parser.
//! Matches: !commandName or !commandName("arg1", 42, true)
//! Mirrors the regex-based parser in commands/index.js.

use anyhow::{bail, Result};
use regex::Regex;
use once_cell::sync::Lazy;

use crate::CommandArg;

static CMD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"!(\w+)(?:\(((?:[^)]*)?)\))?").unwrap()
});

static ARG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"-?\d+(?:\.\d+)?|true|false|"[^"]*"|'[^']*'"#).unwrap()
});

#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub name: String,
    pub raw_args: Vec<String>,
}

/// Find the first command in a message, return its name with ! prefix.
pub fn contains_command(msg: &str) -> Option<String> {
    CMD_RE.find(msg).map(|m| {
        let s = m.as_str();
        let name_end = s.find('(').unwrap_or(s.len());
        format!("!{}", &s[1..name_end])
    })
}

/// Truncate message to just the command (everything after is ignored).
pub fn trunc_command_message(msg: &str) -> &str {
    if let Some(m) = CMD_RE.find(msg) {
        &msg[..m.end()]
    } else {
        msg
    }
}

/// Parse a full command invocation from a message.
pub fn parse_command(msg: &str) -> Result<ParsedCommand> {
    let cap = CMD_RE.captures(msg)
        .ok_or_else(|| anyhow::anyhow!("Command is incorrectly formatted"))?;

    let name = format!("!{}", &cap[1]);
    let raw_args: Vec<String> = if let Some(args_str) = cap.get(2) {
        ARG_RE.find_iter(args_str.as_str())
            .map(|m| {
                let s = m.as_str();
                // strip surrounding quotes
                if (s.starts_with('"') && s.ends_with('"'))
                    || (s.starts_with('\'') && s.ends_with('\''))
                {
                    s[1..s.len()-1].to_string()
                } else {
                    s.to_string()
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(ParsedCommand { name, raw_args })
}

/// Convert a raw string arg to the expected typed CommandArg.
pub fn coerce_arg(raw: &str, expected: &crate::registry::ParamType) -> Result<CommandArg> {
    use crate::registry::ParamType;
    match expected {
        ParamType::Int => raw.parse::<i64>()
            .map(CommandArg::Int)
            .map_err(|_| anyhow::anyhow!("expected int, got {raw:?}")),
        ParamType::Float => raw.parse::<f64>()
            .map(CommandArg::Float)
            .map_err(|_| anyhow::anyhow!("expected float, got {raw:?}")),
        ParamType::Boolean => match raw.to_lowercase().as_str() {
            "true" | "t" | "1" | "on" => Ok(CommandArg::Bool(true)),
            "false" | "f" | "0" | "off" => Ok(CommandArg::Bool(false)),
            _ => bail!("expected boolean, got {raw:?}"),
        },
        ParamType::String | ParamType::BlockName | ParamType::ItemName |
        ParamType::BlockOrItemName => {
            let mut s = raw.to_string();
            // Fix common mistakes: "oak_plank" → "oak_planks"
            if matches!(expected, ParamType::BlockName | ParamType::ItemName | ParamType::BlockOrItemName) {
                if s.ends_with("plank") || s.ends_with("seed") {
                    s.push('s');
                }
            }
            Ok(CommandArg::Text(s))
        }
    }
}
