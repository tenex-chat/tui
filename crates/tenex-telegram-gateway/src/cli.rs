use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::state::TriggerMode;

#[derive(Debug, Parser)]
#[command(name = "tenex-telegram-gateway")]
#[command(about = "Bridge Telegram chats and TENEX agents via a dedicated gateway pubkey")]
pub struct Cli {
    /// Gateway data directory (default: ~/.tenex/telegram-gateway)
    #[arg(long, short = 'd')]
    pub data_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Initialize gateway config interactively
    Init {
        /// Overwrite an existing config file
        #[arg(long)]
        force: bool,
        /// Telegram bot token (otherwise prompted)
        #[arg(long)]
        telegram_bot_token: Option<String>,
        /// Gateway nsec. If omitted, a new key is generated.
        #[arg(long)]
        gateway_nsec: Option<String>,
        /// Relay URL(s). Repeat to provide multiple values.
        #[arg(long = "relay")]
        relay_urls: Vec<String>,
        /// Import relay/trusted-backend settings from ~/.tenex/cli/preferences.json
        #[arg(long, default_value_t = true)]
        import_from_tenex_cli: bool,
        /// Local bind address for the webhook listener
        #[arg(long, default_value = "127.0.0.1:8788")]
        bind_addr: String,
        /// Public HTTPS base URL that Telegram can reach
        #[arg(long)]
        public_base_url: Option<String>,
    },
    /// Run the gateway webhook server and TENEX bridge loop
    Run,
    /// Print the gateway hex pubkey from the configured nsec
    Pubkey,
    /// List projects discovered from TENEX
    Projects {
        /// How long to wait for TENEX sync before listing
        #[arg(long, default_value_t = 8)]
        wait_secs: u64,
    },
    /// List currently online agents for a project
    Agents {
        project_slug: String,
        /// How long to wait for TENEX sync before listing
        #[arg(long, default_value_t = 8)]
        wait_secs: u64,
    },
    /// List chats/topics observed by the webhook runtime
    Chats,
    /// List current Telegram -> TENEX bindings
    Bindings,
    /// Bind a Telegram chat or topic to a TENEX project + agent
    Bind {
        #[arg(long)]
        chat_id: i64,
        #[arg(long)]
        topic_id: Option<i64>,
        #[arg(long)]
        project: String,
        /// Agent name or pubkey. Defaults to the online PM agent.
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, value_enum, default_value_t = TriggerMode::Mention)]
        mode: TriggerMode,
        /// How long to wait for TENEX sync before resolving the project/agent
        #[arg(long, default_value_t = 8)]
        wait_secs: u64,
    },
    /// Remove a Telegram binding
    Unbind {
        #[arg(long)]
        chat_id: i64,
        #[arg(long)]
        topic_id: Option<i64>,
    },
}
