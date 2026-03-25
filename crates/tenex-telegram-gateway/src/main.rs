mod cli;
mod config;
mod gateway;
mod ngrok;
mod state;
mod telegram;
mod tenex;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::Parser;

use crate::cli::{Cli, Commands};
use crate::config::{
    build_init_config, default_gateway_data_dir, parse_gateway_keys, GatewayConfig, InitInputs,
};
use crate::gateway::run_gateway;
use crate::state::{ChatBinding, GatewayStateStore};
use crate::telegram::{TelegramBotIdentity, TelegramClient};
use crate::tenex::TenexContext;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.unwrap_or_else(default_gateway_data_dir);

    match cli.command {
        Commands::Init {
            force,
            telegram_bot_token,
            gateway_nsec,
            relay_urls,
            import_from_tenex_cli,
            bind_addr,
            public_base_url,
        } => {
            let config_path = GatewayConfig::config_path(&data_dir);
            if config_path.exists() && !force {
                return Err(anyhow!(
                    "Config already exists at {}. Re-run with --force to overwrite it.",
                    config_path.display()
                ));
            }

            let config = build_init_config(
                &data_dir,
                InitInputs {
                    telegram_bot_token,
                    gateway_nsec,
                    relay_urls,
                    bind_addr,
                    public_base_url,
                    import_from_tenex_cli,
                },
            )?;

            let telegram = TelegramClient::new(config.telegram_bot_token.clone());
            let bot = TelegramBotIdentity::from_user(
                telegram
                    .get_me()
                    .await
                    .context("Telegram bot token verification failed")?,
            )?;
            let gateway_pubkey = parse_gateway_keys(&config.gateway_nsec)?
                .public_key()
                .to_hex();

            println!("Saved gateway config to {}", config_path.display());
            println!("Bot username: @{}", bot.username);
            println!("Gateway pubkey: {}", gateway_pubkey);
            println!("Gateway pubkey must be whitelisted in TENEX before agent routing will work.");
            println!("Webhook URL: {}", config.webhook_url());
        }
        Commands::Run => {
            let config = load_config(&data_dir)?;
            run_gateway(config, &data_dir).await?;
        }
        Commands::Pubkey => {
            let config = load_config(&data_dir)?;
            let gateway_pubkey = parse_gateway_keys(&config.gateway_nsec)?
                .public_key()
                .to_hex();
            println!("{}", gateway_pubkey);
        }
        Commands::Projects { wait_secs } => {
            let config = load_config(&data_dir)?;
            let mut tenex = TenexContext::connect(&config, &data_dir)?;
            tenex.sync_for(Duration::from_secs(wait_secs)).await?;
            let projects = tenex.list_projects()?;
            if projects.is_empty() {
                println!("No TENEX projects found.");
            } else {
                for project in projects {
                    let online = tenex
                        .online_agents(&project.id)
                        .map(|(_, status)| status.agents.len())
                        .ok();
                    match online {
                        Some(count) => println!(
                            "{}  {}  online_agents={}",
                            project.id,
                            project.a_tag(),
                            count
                        ),
                        None => println!("{}  {}  offline", project.id, project.a_tag()),
                    }
                }
            }
        }
        Commands::Agents {
            project_slug,
            wait_secs,
        } => {
            let config = load_config(&data_dir)?;
            let mut tenex = TenexContext::connect(&config, &data_dir)?;
            tenex.sync_for(Duration::from_secs(wait_secs)).await?;
            let (project, status) = tenex.online_agents(&project_slug)?;
            println!("Project: {} ({})", project.title, project.a_tag());
            for agent in status.agents {
                let role = if agent.is_pm { "pm" } else { "agent" };
                let model = agent.model.unwrap_or_else(|| "-".to_string());
                println!(
                    "{}  {}  role={}  model={}",
                    agent.name, agent.pubkey, role, model
                );
            }
        }
        Commands::Chats => {
            let store = GatewayStateStore::load(&data_dir)?;
            let chats = store.observed_chats();
            if chats.is_empty() {
                println!(
                    "No chats observed yet. Run the gateway and send the bot a message first."
                );
            } else {
                for chat in chats {
                    let scope = chat
                        .message_thread_id
                        .map(|topic| format!("topic={topic}"))
                        .unwrap_or_else(|| "topic=root".to_string());
                    println!(
                        "chat_id={}  {}  type={}  title={}",
                        chat.chat_id, scope, chat.chat_type, chat.chat_title
                    );
                }
            }
        }
        Commands::Bindings => {
            let store = GatewayStateStore::load(&data_dir)?;
            let bindings = store.bindings();
            if bindings.is_empty() {
                println!("No bindings configured.");
            } else {
                for binding in bindings {
                    let scope = binding
                        .message_thread_id
                        .map(|topic| format!("topic={topic}"))
                        .unwrap_or_else(|| "topic=root".to_string());
                    println!(
                        "chat_id={}  {}  project={}  agent={}  mode={}",
                        binding.chat_id,
                        scope,
                        binding.project_slug,
                        binding.agent_name,
                        binding.trigger_mode.as_str()
                    );
                }
            }
        }
        Commands::Bind {
            chat_id,
            topic_id,
            project,
            agent,
            mode,
            wait_secs,
        } => {
            let config = load_config(&data_dir)?;
            let mut tenex = TenexContext::connect(&config, &data_dir)?;
            tenex.sync_for(Duration::from_secs(wait_secs)).await?;
            let resolution = tenex.resolve_binding(&project, agent.as_deref())?;
            let binding = ChatBinding {
                chat_id,
                message_thread_id: topic_id,
                project_slug: resolution.project.id.clone(),
                project_a_tag: resolution.project.a_tag(),
                project_title: resolution.project.title.clone(),
                agent_pubkey: resolution.agent.pubkey.clone(),
                agent_name: resolution.agent.name.clone(),
                trigger_mode: mode,
            };

            let mut store = GatewayStateStore::load(&data_dir)?;
            store.upsert_binding(binding)?;
            println!(
                "Bound chat_id={} topic={:?} -> project={} agent={} mode={}",
                chat_id,
                topic_id,
                resolution.project.id,
                resolution.agent.name,
                mode.as_str()
            );
            if mode == state::TriggerMode::Listen {
                println!(
                    "Listen mode requires Telegram group privacy to be disabled or the bot to have full group message visibility."
                );
            }
        }
        Commands::Unbind { chat_id, topic_id } => {
            let mut store = GatewayStateStore::load(&data_dir)?;
            if store.remove_binding(chat_id, topic_id)? {
                println!(
                    "Removed binding for chat_id={} topic={:?}",
                    chat_id, topic_id
                );
            } else {
                println!(
                    "No binding matched chat_id={} topic={:?}",
                    chat_id, topic_id
                );
            }
        }
    }

    Ok(())
}

fn load_config(data_dir: &PathBuf) -> Result<GatewayConfig> {
    GatewayConfig::load(data_dir).with_context(|| {
        format!(
            "Failed to load gateway config from {}. Run `tenex-telegram-gateway init` first.",
            GatewayConfig::config_path(data_dir).display()
        )
    })
}
