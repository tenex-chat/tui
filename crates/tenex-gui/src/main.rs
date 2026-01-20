use anyhow::Result;
use eframe::egui;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use tenex_core::config::CoreConfig;
use tenex_core::models::PreferencesStorage;
use tenex_core::nostr::{self, DataChange, NostrCommand};
use tenex_core::runtime::{CoreHandle, CoreRuntime};

struct TenexGui {
    core_runtime: CoreRuntime,
    core_handle: CoreHandle,
    data_rx: Option<Receiver<DataChange>>,
    preferences: PreferencesStorage,
    status: String,
    password: String,
    logged_in: bool,
    last_event_count: usize,
    last_event_at: Option<Instant>,
}

impl TenexGui {
    fn new() -> Result<Self> {
        let config = CoreConfig::default();
        let data_dir = config.data_dir.to_str().unwrap_or("tenex_data").to_string();
        let mut core_runtime = CoreRuntime::new(config)?;
        let core_handle = core_runtime.handle();
        let data_rx = core_runtime.take_data_rx();
        let preferences = PreferencesStorage::new(&data_dir);

        let mut app = Self {
            core_runtime,
            core_handle,
            data_rx,
            preferences,
            status: String::new(),
            password: String::new(),
            logged_in: false,
            last_event_count: 0,
            last_event_at: None,
        };

        app.try_auto_login();
        Ok(app)
    }

    fn try_auto_login(&mut self) {
        if !nostr::has_stored_credentials(&self.preferences) {
            self.status = "No stored credentials found".to_string();
            return;
        }
        if nostr::credentials_need_password(&self.preferences) {
            self.status = "Password required".to_string();
            return;
        }

        match nostr::load_unencrypted_keys(&self.preferences) {
            Ok(keys) => {
                let pubkey = nostr::get_current_pubkey(&keys);
                if self
                    .core_handle
                    .send(NostrCommand::Connect {
                        keys: keys.clone(),
                        user_pubkey: pubkey.clone(),
                    })
                    .is_ok()
                {
                    self.logged_in = true;
                    self.status = format!("Connected as {}", &pubkey[..8]);
                } else {
                    self.status = "Failed to send connect command".to_string();
                }
            }
            Err(e) => {
                self.status = format!("Failed to load credentials: {}", e);
            }
        }
    }

    fn unlock_with_password(&mut self) {
        match nostr::load_stored_keys(&self.password, &self.preferences) {
            Ok(keys) => {
                let pubkey = nostr::get_current_pubkey(&keys);
                if self
                    .core_handle
                    .send(NostrCommand::Connect {
                        keys: keys.clone(),
                        user_pubkey: pubkey.clone(),
                    })
                    .is_ok()
                {
                    self.logged_in = true;
                    self.password.clear();
                    self.status = format!("Connected as {}", &pubkey[..8]);
                } else {
                    self.status = "Failed to send connect command".to_string();
                }
            }
            Err(e) => {
                self.status = format!("Unlock failed: {}", e);
            }
        }
    }

    fn poll_core(&mut self) {
        if let Some(note_keys) = self.core_runtime.poll_note_keys() {
            if let Ok(events) = self.core_runtime.process_note_keys(&note_keys) {
                self.last_event_count = events.len();
                self.last_event_at = Some(Instant::now());
            }
        }

        if let Some(rx) = self.data_rx.as_ref() {
            for _ in rx.try_iter() {}
        }
    }
}

impl eframe::App for TenexGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_core();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("TENEX GUI (preview)");

            if !self.status.is_empty() {
                ui.label(&self.status);
            }

            let has_creds = nostr::has_stored_credentials(&self.preferences);
            let needs_password = has_creds && nostr::credentials_need_password(&self.preferences);

            if !self.logged_in {
                if needs_password {
                    ui.horizontal(|ui| {
                        ui.label("Password:");
                        ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
                        if ui.button("Unlock").clicked() {
                            self.unlock_with_password();
                        }
                    });
                } else if has_creds {
                    if ui.button("Connect").clicked() {
                        self.try_auto_login();
                    }
                } else {
                    ui.label("No stored credentials available for login.");
                }
            }

            if let Some(last_event_at) = self.last_event_at {
                ui.label(format!(
                    "Last update: {} events ({:.1}s ago)",
                    self.last_event_count,
                    last_event_at.elapsed().as_secs_f32()
                ));
            }

            let store = self.core_runtime.data_store();
            let store = store.borrow();
            let projects = store.get_projects().len();
            let threads: usize = store
                .get_projects()
                .iter()
                .map(|p| store.get_threads(&p.a_tag()).len())
                .sum();
            let messages: usize = store
                .get_projects()
                .iter()
                .flat_map(|p| store.get_threads(&p.a_tag()))
                .map(|t| store.get_messages(&t.id).len())
                .sum();

            ui.separator();
            ui.label(format!("Projects: {}", projects));
            ui.label(format!("Threads: {}", threads));
            ui.label(format!("Messages: {}", messages));

            if !store.get_projects().is_empty() {
                ui.separator();
                ui.label("Projects (first 10):");
                for project in store.get_projects().iter().take(10) {
                    ui.horizontal(|ui| {
                        ui.label(&project.name);
                        ui.label(project.a_tag());
                    });
                }
            }
        });

        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

impl Drop for TenexGui {
    fn drop(&mut self) {
        self.core_runtime.shutdown();
    }
}

fn main() -> Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "TENEX GUI",
        options,
        Box::new(|_cc| Ok(Box::new(TenexGui::new().expect("Failed to init GUI")))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}
