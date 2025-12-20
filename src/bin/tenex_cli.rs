use clap::{Parser, Subcommand};
use tenex_tui::cli::{run_daemon, send_command, CliCommand};

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

    /// Trigger relay sync
    Sync,

    /// Get daemon status
    Status,

    /// Shutdown the daemon
    Shutdown,
}

fn main() {
    let cli = Cli::parse();

    // Run daemon mode
    if cli.daemon {
        if let Err(e) = run_daemon() {
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
        Some(Commands::Sync) => CliCommand::Sync,
        Some(Commands::Status) => CliCommand::Status,
        Some(Commands::Shutdown) => CliCommand::Shutdown,
        None => {
            // No command - show help
            eprintln!("No command specified. Use --help for usage.");
            std::process::exit(1);
        }
    };

    // Send command to daemon
    if let Err(e) = send_command(command, cli.pretty) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
