//! Entry point. Mirrors main.js + mindcraft.js orchestration.
//!
//! Key difference from JS: no child-process spawning.
//! Each agent is a tokio::task in the same process. IPC via channels.

use std::sync::Arc;
use anyhow::Result;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt};

use mindcraft_config::{Settings, Keys, load_profile};
use mindcraft_llm::prompter::Prompter;
use mindcraft_minecraft::{BotHandle, BotConfig};
use mindcraft_core::agent::Agent;
use mindcraft_server::{run_server, state::{AppState, AgentEntry, AgentCommand}};

#[derive(Parser, Debug)]
#[command(name = "mindcraft", about = "Minecraft AI agent framework in Rust")]
struct Args {
    /// Path to settings JSON
    #[arg(short, long, default_value = "settings.json")]
    settings: String,

    /// Path to API keys JSON
    #[arg(short, long, default_value = "keys.json")]
    keys: String,

    /// Path to static web UI files
    #[arg(long, default_value = "src/mindcraft/public")]
    ui_dir: String,

    /// Only spawn a single named profile (overrides settings.profiles)
    #[arg(short, long)]
    profile: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    let args = Args::parse();
    let mut settings = Settings::load_or_default(&args.settings);
    let keys = Keys::load(&args.keys);

    info!("Starting MindCraft (Rust)");

    // MindServer shared state
    let app_state = AppState::new();

    // Bot output relay: receives (agent_name, message) from all BotHandle instances
    let (server_out_tx, mut server_out_rx) = mpsc::unbounded_channel::<(String, String)>();
    {
        let state = app_state.clone();
        tokio::spawn(async move {
            while let Some((agent, msg)) = server_out_rx.recv().await {
                state.broadcast_output(&agent, &msg);
            }
        });
    }

    // Agent IPC: routes cross-agent messages
    let (agent_msg_tx, mut agent_msg_rx) = mpsc::unbounded_channel::<(String, String)>();
    let state_for_ipc = app_state.clone();
    tokio::spawn(async move {
        while let Some((to_agent, message)) = agent_msg_rx.recv().await {
            if let Some(entry) = state_for_ipc.agents.get(&to_agent) {
                let _ = entry.msg_tx.send(AgentCommand::SendMessage {
                    from: "agent".to_string(),
                    message,
                });
            }
        }
    });

    // Spawn agents
    let profiles: Vec<String> = if let Some(p) = args.profile {
        vec![p]
    } else {
        settings.profiles.clone()
    };

    for (idx, profile_path) in profiles.iter().enumerate() {
        let profile = match load_profile(profile_path, &settings.base_profile) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to load profile {profile_path}: {e}");
                continue;
            }
        };

        let agent_name = profile.name.clone();
        info!("Spawning agent: {agent_name}");

        let prompter = match Prompter::new(profile.clone(), &settings, &keys) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to create prompter for {agent_name}: {e}");
                continue;
            }
        };

        let bot_cfg = BotConfig::from_settings(agent_name.clone(), &settings);
        let bot = BotHandle::new(
            bot_cfg,
            server_out_tx.clone(),
            agent_msg_tx.clone(),
        );

        let blocked = settings.blocked_actions.clone();
        let agent = Agent::new(
            agent_name.clone(),
            settings.clone(),
            prompter,
            bot.clone(),
            blocked,
        );

        // Per-agent command channel
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<AgentCommand>();
        let viewer_port = 3000 + idx as u16;

        app_state.agents.insert(agent_name.clone(), AgentEntry {
            name: agent_name.clone(),
            in_game: false,
            viewer_port,
            msg_tx: cmd_tx,
            full_state: None,
        });

        // Agent task
        let agent_clone = agent.clone();
        let load_mem = settings.load_memory;
        let init_msg = settings.init_message.clone();
        tokio::spawn(async move {
            // Connect bot to MC server
            if let Err(e) = bot.connect().await {
                error!("Bot {}: failed to connect: {e}", agent_clone.name);
                return;
            }

            // Optionally load memory
            if load_mem {
                agent_clone.history.lock().await.load();
            }

            // Send init message once spawned
            if let Some(msg) = init_msg {
                agent_clone.send_message("system", msg, None);
            }

            // Run agent loop
            agent_clone.run().await;
        });

        // Agent command dispatcher
        let agent_for_cmd = agent.clone();
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    AgentCommand::SendMessage { from, message } => {
                        agent_for_cmd.send_message(from, message, None);
                    }
                    AgentCommand::Stop => {
                        agent_for_cmd.clean_kill("Stopped by server.", 0).await;
                    }
                    AgentCommand::Restart => {
                        agent_for_cmd.clean_kill("Restarting.", 1).await;
                    }
                    AgentCommand::GetFullState { reply_json: _ } => {
                        // Full state polling for UI — serialized separately
                    }
                }
            }
        });
    }

    app_state.broadcast_status();

    // Run MindServer (blocks until shutdown)
    run_server(settings.mindserver_port, &args.ui_dir, app_state).await?;

    Ok(())
}
