mod clipboard;
mod input;
mod render;
mod runtime;
mod ui;

pub use tenex_core::models;
pub use tenex_core::nostr;
pub use tenex_core::store;
pub use tenex_core::streaming;

use anyhow::Result;
use clap::Parser;
use nostr::NostrCommand;
use nostr_sdk::prelude::*;
use tenex_core::config::CoreConfig;
use tenex_core::runtime::CoreRuntime;

use crate::runtime::run_app;
use ui::views::login::LoginStep;
use ui::{App, InputMode, View};

/// TENEX TUI Client
#[derive(Parser, Debug)]
#[command(name = "tenex-tui")]
#[command(about = "TENEX Terminal User Interface Client")]
struct Args {
    /// Use this nsec directly instead of loading from file.
    /// WARNING: This exposes your secret key in shell history and process lists.
    /// Prefer TENEX_NSEC environment variable for safer usage.
    #[arg(long)]
    nsec: Option<String>,
}

/// Authentication source for the nsec key
enum NsecSource {
    /// Provided via CLI argument
    CliArg(String),
    /// Provided via TENEX_NSEC environment variable
    EnvVar(String),
    /// Load from stored credentials (file-based)
    Stored,
}

/// Result of attempting to resolve authentication
enum AuthResult {
    /// Successfully authenticated, switch to Home view
    Success,
    /// Need to show login UI with this step
    ShowLogin(LoginStep, Option<String>),
}

/// Determine the nsec source based on CLI args and environment
fn resolve_nsec_source(args: &Args) -> NsecSource {
    // CLI argument takes highest precedence
    if let Some(ref nsec) = args.nsec {
        return NsecSource::CliArg(nsec.clone());
    }

    // Environment variable is next
    if let Ok(nsec) = std::env::var("TENEX_NSEC") {
        if !nsec.is_empty() {
            return NsecSource::EnvVar(nsec);
        }
    }

    // Fall back to stored credentials
    NsecSource::Stored
}

/// Connect and initialize the app with authenticated keys
fn connect_and_init(
    app: &mut App,
    core_handle: &tenex_core::runtime::CoreHandle,
    keys: Keys,
) -> Result<(), String> {
    let user_pubkey = nostr::get_current_pubkey(&keys);
    app.keys = Some(keys.clone());
    app.data_store.borrow_mut().set_user_pubkey(user_pubkey.clone());

    core_handle
        .send(NostrCommand::Connect {
            keys: keys.clone(),
            user_pubkey: user_pubkey.clone(),
            response_tx: None,
        })
        .map_err(|e| format!("Failed to connect: {}", e))?;

    app.view = View::Home;
    app.load_filter_preferences();
    app.init_trusted_backends();
    Ok(())
}

/// Resolve authentication based on nsec source and stored credentials
fn resolve_authentication(
    app: &mut App,
    core_handle: &tenex_core::runtime::CoreHandle,
    nsec_source: NsecSource,
) -> AuthResult {
    match nsec_source {
        NsecSource::CliArg(nsec) | NsecSource::EnvVar(nsec) => {
            match SecretKey::parse(&nsec) {
                Ok(secret_key) => {
                    let keys = Keys::new(secret_key);
                    match connect_and_init(app, core_handle, keys.clone()) {
                        Ok(()) => AuthResult::Success,
                        Err(e) => {
                            AuthResult::ShowLogin(LoginStep::Nsec, Some(e))
                        }
                    }
                }
                Err(e) => {
                    AuthResult::ShowLogin(
                        LoginStep::Nsec,
                        Some(format!("Invalid nsec provided: {}", e)),
                    )
                }
            }
        }
        NsecSource::Stored => {
            // Check credentials state upfront to avoid borrow conflicts
            let has_creds = nostr::has_stored_credentials(&app.preferences.borrow());
            let needs_password = has_creds && nostr::credentials_need_password(&app.preferences.borrow());

            if !has_creds {
                // No credentials - show nsec prompt
                AuthResult::ShowLogin(LoginStep::Nsec, None)
            } else if needs_password {
                // Password required - show unlock prompt
                AuthResult::ShowLogin(LoginStep::Unlock, None)
            } else {
                // No password - auto-login with unencrypted credentials
                // Load keys first, then connect (to avoid borrow conflicts)
                let keys_result = nostr::load_unencrypted_keys(&app.preferences.borrow());
                match keys_result {
                    Ok(keys) => {
                        match connect_and_init(app, core_handle, keys.clone()) {
                            Ok(()) => AuthResult::Success,
                            Err(e) => {
                                AuthResult::ShowLogin(LoginStep::Nsec, Some(e))
                            }
                        }
                    }
                    Err(e) => {
                        AuthResult::ShowLogin(
                            LoginStep::Nsec,
                            Some(format!("Failed to load credentials: {}", e)),
                        )
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Set up panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Restore terminal before showing panic
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        );
        // Print panic info to stderr
        eprintln!("\n\n=== PANIC ===");
        eprintln!("{}", panic_info);
        eprintln!("=============\n");
        // Call original hook
        original_hook(panic_info);
    }));

    let config = CoreConfig::default();
    let data_dir = config.data_dir.to_str().unwrap_or("tenex_data").to_string();
    let mut core_runtime = CoreRuntime::new(config)?;
    let data_store = core_runtime.data_store();
    let db = core_runtime.database();
    let event_stats = core_runtime.event_stats();
    let subscription_stats = core_runtime.subscription_stats();
    let negentropy_stats = core_runtime.negentropy_stats();
    let mut app = App::new(db.clone(), data_store, event_stats, subscription_stats, negentropy_stats, &data_dir);
    let mut terminal = ui::init_terminal()?;
    let core_handle = core_runtime.handle();
    let data_rx = core_runtime
        .take_data_rx()
        .ok_or_else(|| anyhow::anyhow!("Core runtime already has active data receiver"))?;
    app.set_core_handle(core_handle.clone(), data_rx);

    // Resolve authentication: CLI arg > env var > stored credentials
    let nsec_source = resolve_nsec_source(&args);
    let mut login_step = match resolve_authentication(&mut app, &core_handle, nsec_source) {
        AuthResult::Success => LoginStep::Nsec, // Won't be shown since view is Home
        AuthResult::ShowLogin(step, error_msg) => {
            if let Some(msg) = error_msg {
                app.set_status(&msg);
            }
            app.input_mode = InputMode::Editing;
            step
        }
    };
    let mut pending_nsec: Option<String> = None;

    let result = run_app(
        &mut terminal,
        &mut app,
        &mut core_runtime,
        &mut login_step,
        &mut pending_nsec,
    )
    .await;

    core_runtime.shutdown();

    ui::restore_terminal()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}
