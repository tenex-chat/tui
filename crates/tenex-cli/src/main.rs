use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tenex_cli::cli::{is_daemon_running, run_daemon, send_command, socket_path, CliCommand, CliConfig};
use tenex_core::config::CoreConfig;
use tenex_core::slug::{validate_slug, SlugValidation};

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
        /// Skill event IDs to attach (can be specified multiple times).
        /// Must be 64-character hex strings (Nostr event IDs).
        #[arg(long, short = 'S')]
        skill: Vec<String>,
        /// Nudge event IDs to attach (can be specified multiple times).
        /// Must be 64-character hex strings (Nostr event IDs).
        #[arg(long, short = 'N')]
        nudge: Vec<String>,
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
        /// Skill event IDs to attach (can be specified multiple times).
        /// Must be 64-character hex strings (Nostr event IDs).
        #[arg(long, short = 'S')]
        skill: Vec<String>,
        /// Nudge event IDs to attach (can be specified multiple times).
        /// Must be 64-character hex strings (Nostr event IDs).
        #[arg(long, short = 'N')]
        nudge: Vec<String>,
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

    /// List all MCP tools (kind:4200 events)
    ListMCPTools,

    /// List all skills (kind:4202 events).
    /// Use `--skill <ID>` with send-message or create-thread to attach skills.
    ListSkills,

    /// List all nudges (kind:4201 events).
    /// Use `--nudge <ID>` with send-message or create-thread to attach nudges.
    ListNudges,

    /// Show detailed project information (from kind:24010)
    ShowProject {
        /// Project slug (d-tag)
        project_slug: String,
        /// Wait for project status (24010 event) before proceeding
        #[arg(long, short = 'W')]
        wait: bool,
    },

    /// Save a project (create new or update existing) (kind:31933 event)
    ///
    /// This command publishes a replaceable project event. If a project with the same
    /// slug (d-tag) already exists, it will be replaced/updated. This allows you to:
    /// - Create a new project with a unique slug
    /// - Update an existing project's name, description, agents, or MCP tools
    ///
    /// The slug serves as the project's unique identifier (d-tag in NIP-33).
    /// Slugs are normalized: lowercase, non-alphanumeric chars become dashes.
    #[command(alias = "create-project")]
    SaveProject {
        /// Project slug (d-tag) - unique identifier for the project.
        /// If omitted, generated from the project name.
        /// Use the same slug to update an existing project.
        /// Slugs are normalized: trimmed, lowercased, non-alphanumeric chars become dashes.
        #[arg(long, short = 's')]
        slug: Option<String>,
        /// Project name/title
        #[arg(long, short = 'n')]
        name: String,
        /// Project description
        #[arg(long, short = 'd', default_value = "")]
        description: String,
        /// Agent IDs to include in the project (can be specified multiple times).
        /// Updates replace all existing agents - include all desired agents.
        #[arg(long, short = 'a')]
        agent: Vec<String>,
        /// MCP tool event IDs to include in the project (can be specified multiple times).
        /// Must be 64-character hex strings (32-byte event IDs).
        /// Updates replace all existing MCP tools - include all desired tools.
        #[arg(long = "mcp-event-id", short = 'm')]
        mcp_tool_ids: Vec<String>,
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
        Some(Commands::SendMessage { project_slug, thread_id, recipient_slug, wait, wait_for_project, skill, nudge, message }) => {
            CliCommand::SendMessage {
                project_slug,
                thread_id,
                recipient_slug,
                content: message.join(" "),
                wait_secs: wait,
                wait_for_project,
                skill_ids: validate_skill_ids(skill),
                nudge_ids: validate_nudge_ids(nudge),
            }
        }
        Some(Commands::CreateThread { project_slug, recipient_slug, wait, wait_for_project, skill, nudge, message }) => {
            CliCommand::CreateThread {
                project_slug,
                recipient_slug,
                content: message.join(" "),
                wait_secs: wait,
                wait_for_project,
                skill_ids: validate_skill_ids(skill),
                nudge_ids: validate_nudge_ids(nudge),
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
        Some(Commands::ListMCPTools) => CliCommand::ListMCPTools,
        Some(Commands::ListSkills) => CliCommand::ListSkills,
        Some(Commands::ListNudges) => CliCommand::ListNudges,
        Some(Commands::ShowProject { project_slug, wait }) => CliCommand::ShowProject { project_slug, wait_for_project: wait },
        Some(Commands::SaveProject { slug, name, description, agent, mcp_tool_ids }) => {
            // Validate and normalize name
            let name_trimmed = name.trim();
            if name_trimmed.is_empty() {
                eprintln!("Error: Project name cannot be empty or whitespace-only");
                std::process::exit(1);
            }

            // Validate and normalize slug (or generate from name)
            let (final_slug, slug_was_generated) = if let Some(ref user_slug) = slug {
                // User provided a slug - validate and normalize it
                match validate_slug(user_slug) {
                    SlugValidation::Valid(normalized) => (normalized, false),
                    SlugValidation::Empty => {
                        eprintln!("Error: Slug cannot be empty or whitespace-only");
                        eprintln!("Hint: Either provide a valid slug with --slug, or omit it to auto-generate from the name");
                        std::process::exit(1);
                    }
                    SlugValidation::OnlyDashes => {
                        eprintln!("Error: Slug must contain at least one alphanumeric character");
                        eprintln!("Hint: Slugs are normalized to lowercase with dashes. Got: '{}'", user_slug);
                        std::process::exit(1);
                    }
                }
            } else {
                // No slug provided - generate from name
                match validate_slug(name_trimmed) {
                    SlugValidation::Valid(normalized) => (normalized, true),
                    SlugValidation::Empty | SlugValidation::OnlyDashes => {
                        eprintln!("Error: Cannot generate slug from name '{}' - name must contain at least one alphanumeric character", name_trimmed);
                        std::process::exit(1);
                    }
                }
            };

            // Validate MCP event IDs
            let validated_mcp_tool_ids: Vec<String> = mcp_tool_ids
                .into_iter()
                .map(|id| {
                    let trimmed = id.trim().to_string();
                    if trimmed.is_empty() {
                        eprintln!("Error: MCP event ID cannot be empty");
                        std::process::exit(1);
                    }
                    if trimmed.len() != 64 {
                        eprintln!("Error: MCP event ID must be 64 hex characters (got {} characters): {}", trimmed.len(), trimmed);
                        std::process::exit(1);
                    }
                    if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                        eprintln!("Error: MCP event ID must contain only hex characters: {}", trimmed);
                        std::process::exit(1);
                    }
                    trimmed
                })
                .collect();

            // Show generated slug feedback if it was auto-generated
            if slug_was_generated {
                eprintln!("Using generated slug: {}", final_slug);
            }

            CliCommand::SaveProject {
                slug: Some(final_slug),
                name: name_trimmed.to_string(),
                description,
                agent_ids: agent,
                mcp_tool_ids: validated_mcp_tool_ids,
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

/// Validate and normalize skill IDs.
/// - Trims whitespace from each ID
/// - Filters out empty/whitespace-only IDs
/// - Deduplicates IDs
/// - Validates 64-character hex format
/// Returns the validated IDs or exits with error if any ID is invalid.
fn validate_skill_ids(skill_ids: Vec<String>) -> Vec<String> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut validated = Vec::new();

    for id in skill_ids {
        let trimmed = id.trim().to_string();

        // Skip empty/whitespace-only IDs
        if trimmed.is_empty() {
            continue;
        }

        // Skip duplicates
        if seen.contains(&trimmed) {
            continue;
        }

        // Validate 64-character hex format
        if trimmed.len() != 64 {
            eprintln!("Error: Skill event ID must be 64 hex characters (got {} characters): {}", trimmed.len(), trimmed);
            std::process::exit(1);
        }
        if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            eprintln!("Error: Skill event ID must contain only hex characters: {}", trimmed);
            std::process::exit(1);
        }

        seen.insert(trimmed.clone());
        validated.push(trimmed);
    }

    validated
}

/// Validate and normalize nudge IDs.
/// - Trims whitespace from each ID
/// - Filters out empty/whitespace-only IDs
/// - Deduplicates IDs
/// - Validates 64-character hex format
/// Returns the validated IDs or exits with error if any ID is invalid.
fn validate_nudge_ids(nudge_ids: Vec<String>) -> Vec<String> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut validated = Vec::new();

    for id in nudge_ids {
        let trimmed = id.trim().to_string();

        // Skip empty/whitespace-only IDs
        if trimmed.is_empty() {
            continue;
        }

        // Skip duplicates
        if seen.contains(&trimmed) {
            continue;
        }

        // Validate 64-character hex format
        if trimmed.len() != 64 {
            eprintln!("Error: Nudge event ID must be 64 hex characters (got {} characters): {}", trimmed.len(), trimmed);
            std::process::exit(1);
        }
        if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            eprintln!("Error: Nudge event ID must contain only hex characters: {}", trimmed);
            std::process::exit(1);
        }

        seen.insert(trimmed.clone());
        validated.push(trimmed);
    }

    validated
}
