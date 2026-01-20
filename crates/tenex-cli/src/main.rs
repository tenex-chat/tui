use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tenex_cli::cli::{is_daemon_running, run_daemon, send_command, socket_path, CliCommand, CliConfig};

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

    /// Path to JSON config file (contains socketPath, credentials)
    #[arg(long, short = 'c')]
    config: Option<PathBuf>,

    /// JSON config passed internally (used when spawning daemon)
    #[arg(long, hide = true)]
    config_json: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all projects
    ListProjects,

    /// List threads in a project
    ListThreads {
        /// Project ID (a-tag format)
        project_id: String,
    },

    /// List messages in a thread
    ListMessages {
        /// Thread ID (event ID)
        thread_id: String,
    },

    /// Get full state dump
    GetState,

    /// Send a message to a thread
    SendMessage {
        /// Thread ID (event ID)
        thread_id: String,
        /// Message content
        content: String,
    },

    /// Create a new thread in a project
    CreateThread {
        /// Project ID (a-tag format)
        project_id: String,
        /// Thread title
        title: String,
    },

    /// Boot/start a project (sends kind 24000 event)
    BootProject {
        /// Project ID (a-tag format)
        project_id: String,
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
}

fn main() {
    let cli = Cli::parse();

    // Load config from file or JSON string
    let config = load_config(&cli);

    // Run daemon mode
    if cli.daemon {
        if let Err(e) = run_daemon(config) {
            eprintln!("Daemon error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Convert subcommand to CliCommand
    let command = match cli.command {
        Some(Commands::ListProjects) => CliCommand::ListProjects,
        Some(Commands::ListThreads { project_id }) => CliCommand::ListThreads { project_id },
        Some(Commands::ListMessages { thread_id }) => CliCommand::ListMessages { thread_id },
        Some(Commands::GetState) => CliCommand::GetState,
        Some(Commands::SendMessage { thread_id, content }) => {
            CliCommand::SendMessage { thread_id, content }
        }
        Some(Commands::CreateThread { project_id, title }) => {
            CliCommand::CreateThread { project_id, title }
        }
        Some(Commands::BootProject { project_id }) => CliCommand::BootProject { project_id },
        Some(Commands::Status { running }) => {
            if running {
                // Quick check without auto-starting daemon
                let is_running = is_daemon_running(config.as_ref());
                let path = socket_path(config.as_ref());
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
        Some(Commands::CreateProject { name, description, agent }) => {
            CliCommand::CreateProject {
                name,
                description,
                agent_ids: agent,
            }
        }
        None => {
            // No command - show help
            eprintln!("No command specified. Use --help for usage.");
            std::process::exit(1);
        }
    };

    // Send command to daemon
    if let Err(e) = send_command(command, cli.pretty, config) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Load configuration from file or JSON string
fn load_config(cli: &Cli) -> Option<CliConfig> {
    // Priority: --config-json (internal) > --config (file)
    if let Some(ref json) = cli.config_json {
        match CliConfig::from_json(json) {
            Ok(config) => return Some(config),
            Err(e) => {
                eprintln!("Warning: Failed to parse config JSON: {}", e);
            }
        }
    }

    if let Some(ref path) = cli.config {
        match CliConfig::load(path) {
            Ok(config) => return Some(config),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    None
}
