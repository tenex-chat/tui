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
use nostr::NostrCommand;
use tenex_core::config::CoreConfig;
use tenex_core::runtime::CoreRuntime;

use crate::runtime::run_app;
use ui::views::login::LoginStep;
use ui::{App, InputMode, View};

#[tokio::main]
async fn main() -> Result<()> {
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

    let mut core_runtime = CoreRuntime::new(CoreConfig::default())?;
    let data_store = core_runtime.data_store();
    let db = core_runtime.database();
    let mut app = App::new(db.clone(), data_store);
    let mut terminal = ui::init_terminal()?;
    let core_handle = core_runtime.handle();
    let data_rx = core_runtime
        .take_data_rx()
        .ok_or_else(|| anyhow::anyhow!("Core runtime already has active data receiver"))?;
    app.set_core_handle(core_handle.clone(), data_rx);

    let mut login_step = if nostr::has_stored_credentials(&app.db.credentials_conn()) {
        if nostr::credentials_need_password(&app.db.credentials_conn()) {
            // Password required - show unlock prompt with autofocus
            app.input_mode = InputMode::Editing;
            LoginStep::Unlock
        } else {
            // No password - auto-login with unencrypted credentials
            match nostr::load_unencrypted_keys(&app.db.credentials_conn()) {
                Ok(keys) => {
                    let user_pubkey = nostr::get_current_pubkey(&keys);
                    app.keys = Some(keys.clone());
                    app.data_store.borrow_mut().set_user_pubkey(user_pubkey.clone());

                    if let Err(e) = core_handle.send(NostrCommand::Connect {
                        keys: keys.clone(),
                        user_pubkey: user_pubkey.clone(),
                    }) {
                        app.set_status(&format!("Failed to connect: {}", e));
                        LoginStep::Nsec
                    } else if let Err(e) = core_handle.send(NostrCommand::Sync) {
                        app.set_status(&format!("Failed to sync: {}", e));
                        LoginStep::Nsec
                    } else {
                        app.view = View::Home;
                        app.load_filter_preferences();
                        LoginStep::Nsec // Won't be shown since view is Home
                    }
                }
                Err(e) => {
                    app.set_status(&format!("Failed to load credentials: {}", e));
                    LoginStep::Nsec
                }
            }
        }
    } else {
        LoginStep::Nsec
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
