use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tenex_cli::cli::{is_daemon_running, run_daemon, send_command, socket_path, CliCommand, CliConfig};
use tenex_core::config::CoreConfig;

#[derive(Parser)]
#[command(name = "tenex-cli")]
#[command(about = "CLI interface for tenex")]
struct Cli {
    /// Start daemon in foreground
    #[arg(long)]
    daemon: bool,

    /// Pretty-print JSON output
    #[arg(long, short)]
    pretty: bool,

    /// Data directory for config, socket, database, logs, pid (default: ~/.tenex/cli)
    #[arg(long, short = 'd')]
    data_dir: Option<PathBuf>,

    /// Enable HTTP server (OpenAI-compatible API)
    #[arg(long)]
    http: bool,

    /// HTTP server bind address (requires --http)
    #[arg(long, default_value = "127.0.0.1:8080")]
    http_bind: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all projects
    ListProjects,

    /// List threads in a project
    ListThreads {
        /// Project slug (d-tag)
        project_slug: String,
        /// Wait for project status (24010 event) before proceeding
        #[arg(long, short = 'W')]
        wait: bool,
    },

    /// List agents in a project
    ListAgents {
        /// Project slug (d-tag)
        project_slug: String,
        /// Wait for project status (24010 event) before proceeding
        #[arg(long, short = 'W')]
        wait: bool,
    },

    /// List messages in a thread
    ListMessages {
        /// Thread ID (event ID)
        thread_id: String,
    },

    /// Get full state dump
    GetState,

    /// Send a message to a thread (with recipient targeting)
    SendMessage {
        /// Project slug (d-tag)
        project_slug: String,
        /// Thread ID (event ID)
        thread_id: String,
        /// Agent slug (d-tag) within the project to target
        recipient_slug: String,
        /// Wait for agent reply (max seconds to wait)
        #[arg(long, short)]
        wait: Option<u64>,
        /// Wait for project status (24010 event) before proceeding
        #[arg(long, short = 'W')]
        wait_for_project: bool,
        /// Message content (all remaining arguments are joined)
        #[arg(trailing_var_arg = true, num_args = 1..)]
        message: Vec<String>,
    },

    /// Create a new thread in a project (with recipient targeting)
    CreateThread {
        /// Project slug (d-tag)
        project_slug: String,
        /// Agent slug (d-tag) within the project to target
        recipient_slug: String,
        /// Wait for agent reply (max seconds to wait)
        #[arg(long, short)]
        wait: Option<u64>,
        /// Wait for project status (24010 event) before proceeding
        #[arg(long, short = 'W')]
        wait_for_project: bool,
        /// Message content (all remaining arguments are joined)
        #[arg(trailing_var_arg = true, num_args = 1..)]
        message: Vec<String>,
    },

    /// Boot/start a project (sends kind 24000 event)
    BootProject {
        /// Project slug (d-tag)
        project_slug: String,
        /// Wait until the project is online
        #[arg(long, short)]
        wait: bool,
    },

    /// Get daemon status
    Status {
        /// Quick check if daemon is running (doesn't auto-start daemon)
        #[arg(long)]
        running: bool,
    },

    /// Shutdown the daemon
    Shutdown,

    /// List all agent definitions (kind:4199 events)
    ListAgentDefinitions,

    /// Show detailed project information (from kind:24010)
    ShowProject {
        /// Project slug (d-tag)
        project_slug: String,
        /// Wait for project status (24010 event) before proceeding
        #[arg(long, short = 'W')]
        wait: bool,
    },

    /// Create a new project (kind:31933 event)
    CreateProject {
        /// Project name
        #[arg(long, short = 'n')]
        name: String,
        /// Project description
        #[arg(long, short = 'd', default_value = "")]
        description: String,
        /// Agent IDs to include in the project (can be specified multiple times)
        #[arg(long, short = 'a')]
        agent: Vec<String>,
    },

    /// Set agent settings (publishes kind:24020 event to override model/tools)
    SetAgentSettings {
        /// Project slug (d-tag)
        project_slug: String,
        /// Agent slug (name) within the project
        agent_slug: String,
        /// Model to use for this agent
        model: String,
        /// Tools to enable for this agent (can be specified multiple times)
        #[arg(long, short = 't')]
        tool: Vec<String>,
        /// Wait for project status (24010 event) before proceeding
        #[arg(long, short = 'W')]
        wait_for_project: bool,
        /// Wait for confirmation via updated kind:24010 event
        #[arg(long, short)]
        wait: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    // Determine data directory
    let data_dir = cli.data_dir.clone().unwrap_or_else(CoreConfig::default_data_dir);

    // Load config from data_dir/config.json if exists
    let config = load_config(&data_dir);

    // Run daemon mode
    if cli.daemon {
        if let Err(e) = run_daemon(data_dir, config, cli.http, cli.http_bind) {
            eprintln!("Daemon error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Convert subcommand to CliCommand
    let command = match cli.command {
        Some(Commands::ListProjects) => CliCommand::ListProjects,
        Some(Commands::ListThreads { project_slug, wait }) => CliCommand::ListThreads { project_slug, wait_for_project: wait },
        Some(Commands::ListAgents { project_slug, wait }) => CliCommand::ListAgents { project_slug, wait_for_project: wait },
        Some(Commands::ListMessages { thread_id }) => CliCommand::ListMessages { thread_id },
        Some(Commands::GetState) => CliCommand::GetState,
        Some(Commands::SendMessage { project_slug, thread_id, recipient_slug, wait, wait_for_project, message }) => {
            CliCommand::SendMessage {
                project_slug,
                thread_id,
                recipient_slug,
                content: message.join(" "),
                wait_secs: wait,
                wait_for_project,
            }
        }
        Some(Commands::CreateThread { project_slug, recipient_slug, wait, wait_for_project, message }) => {
            CliCommand::CreateThread {
                project_slug,
                recipient_slug,
                content: message.join(" "),
                wait_secs: wait,
                wait_for_project,
            }
        }
        Some(Commands::BootProject { project_slug, wait }) => CliCommand::BootProject { project_slug, wait },
        Some(Commands::Status { running }) => {
            if running {
                // Quick check without auto-starting daemon
                let is_running = is_daemon_running(&data_dir);
                let path = socket_path(&data_dir);
                if cli.pretty {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "running": is_running,
                            "socket_path": path.display().to_string(),
                        }))
                        .unwrap()
                    );
                } else {
                    println!(
                        "{}",
                        serde_json::to_string(&serde_json::json!({
                            "running": is_running,
                            "socket_path": path.display().to_string(),
                        }))
                        .unwrap()
                    );
                }
                std::process::exit(if is_running { 0 } else { 1 });
            }
            CliCommand::Status
        }
        Some(Commands::Shutdown) => CliCommand::Shutdown,
        Some(Commands::ListAgentDefinitions) => CliCommand::ListAgentDefinitions,
        Some(Commands::ShowProject { project_slug, wait }) => CliCommand::ShowProject { project_slug, wait_for_project: wait },
        Some(Commands::CreateProject { name, description, agent }) => {
            CliCommand::CreateProject {
                name,
                description,
                agent_ids: agent,
            }
        }
        Some(Commands::SetAgentSettings { project_slug, agent_slug, model, tool, wait_for_project, wait }) => {
            CliCommand::SetAgentSettings {
                project_slug,
                agent_slug,
                model,
                tools: tool,
                wait_for_project,
                wait,
            }
        }
        None => {
            // No command - show help
            eprintln!("No command specified. Use --help for usage.");
            std::process::exit(1);
        }
    };

    // Send command to daemon
    if let Err(e) = send_command(command, cli.pretty, &data_dir, config) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Load configuration from data_dir/config.json if it exists
fn load_config(data_dir: &std::path::Path) -> Option<CliConfig> {
    let config_path = data_dir.join("config.json");
    if config_path.exists() {
        match CliConfig::load(&config_path) {
            Ok(config) => return Some(config),
            Err(e) => {
                eprintln!("Warning: Failed to load config: {}", e);
            }
        }
    }
    None
}
