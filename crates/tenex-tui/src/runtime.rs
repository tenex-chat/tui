use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use futures::StreamExt;
use std::time::Duration;
use tracing::{debug, info_span};

use tenex_core::events::CoreEvent;
use tenex_core::runtime::CoreRuntime;

use crate::clipboard::{handle_clipboard_paste, handle_image_file_paste, UploadResult};
use crate::input::handle_key;
use crate::render::render;
use crate::ui::views::login::LoginStep;
use crate::ui::{App, InputMode, Tui, View};

pub(crate) async fn run_app(
    terminal: &mut Tui,
    app: &mut App,
    core_runtime: &mut CoreRuntime,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    // Create async event stream for terminal events
    let mut event_stream = EventStream::new();

    // Create a tick interval for regular updates (data channel polling, etc.)
    let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

    // Channel for receiving upload results from background tasks
    let (upload_tx, mut upload_rx) = tokio::sync::mpsc::channel::<UploadResult>(10);

    let mut loop_count: u64 = 0;
    while app.running {
        loop_count += 1;
        if loop_count % 100 == 0 {
            debug!("Event loop iteration {}", loop_count);
        }

        // Render
        let _span = info_span!("render").entered();
        terminal.draw(|f| render(f, app, login_step))?;

        // Wait for events using tokio::select!
        tokio::select! {
            // Terminal UI events
            maybe_event = event_stream.next() => {
                debug!("Received terminal event");
                if let Some(Ok(event)) = maybe_event {
                    match event {
                        Event::Key(key) if key.kind == KeyEventKind::Press => {
                            // Handle Ctrl+C for quit
                            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                if app.pending_quit {
                                    // Second Ctrl+C - quit immediately
                                    app.quit();
                                } else {
                                    // First Ctrl+C - set pending (footer shows warning)
                                    app.pending_quit = true;
                                }
                            } else if key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                // Ctrl+V - check clipboard for image
                                app.pending_quit = false;
                                if app.view == View::Chat && app.input_mode == InputMode::Editing {
                                    if let Some(keys) = app.keys.clone() {
                                        handle_clipboard_paste(app, &keys, upload_tx.clone());
                                    }
                                }
                            } else if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                // Ctrl+T - open command palette
                                app.pending_quit = false;
                                app.open_command_palette();
                            } else {
                                // Any other key clears pending quit state
                                app.pending_quit = false;
                                let _span = info_span!("handle_key", key = ?key.code).entered();
                                handle_key(app, key, login_step, pending_nsec)?;
                            }
                        }
                        Event::Mouse(mouse) => {
                            // Handle mouse scroll in Chat view
                            if app.view == View::Chat {
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        app.scroll_up(3);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        app.scroll_down(3);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Event::Paste(text) => {
                            // Handle paste event - only in Chat view with editing mode
                            if app.view == View::Chat && app.input_mode == InputMode::Editing {
                                if app.showing_attachment_modal {
                                    app.attachment_modal_editor_mut().handle_paste(&text);
                                } else {
                                    // Check if pasted text is an image file path (drag & drop)
                                    if let Some(keys) = app.keys.clone() {
                                        if !handle_image_file_paste(app, &text, &keys, upload_tx.clone()) {
                                            // Not an image file - regular paste
                                            app.chat_editor.handle_paste(&text);
                                            app.save_chat_draft();
                                        }
                                    } else {
                                        app.chat_editor.handle_paste(&text);
                                        app.save_chat_draft();
                                    }
                                }
                            } else if app.input_mode == InputMode::Editing {
                                // Simple paste for login/threads views
                                for c in text.chars() {
                                    app.enter_char(c);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // nostrdb notifications - events are ready to query
            Some(note_keys) = core_runtime.next_note_keys() => {
                debug!("core_runtime received {} note keys", note_keys.len());
                let _span = info_span!("ndb_subscription", note_count = note_keys.len()).entered();
                let events = core_runtime.process_note_keys(&note_keys)?;
                handle_core_events(app, events);

                // Check for pending new thread and navigate to it if found
                check_pending_new_thread(app);

                debug!("core_runtime processing complete");
            }

            // Tick for regular updates (data channel polling for non-message updates)
            _ = tick_interval.tick() => {
                let _span = info_span!("check_data_updates").entered();
                app.tick(); // Increment frame counter for animations
                app.check_for_data_updates()?;
            }

            // Handle upload results from background tasks
            Some(result) = upload_rx.recv() => {
                match result {
                    UploadResult::Success(url) => {
                        let id = app.chat_editor.add_image_attachment(url);
                        let marker = format!("[Image #{}] ", id);
                        for c in marker.chars() {
                            app.chat_editor.insert_char(c);
                        }
                        app.save_chat_draft();
                        app.dismiss_notification();
                    }
                    UploadResult::Error(msg) => {
                        app.set_status(&msg);
                    }
                }
            }
        }
    }
    Ok(())
}

fn handle_core_events(app: &mut App, events: Vec<CoreEvent>) {
    for event in events {
        match event {
            CoreEvent::Message(message) => {
                let thread_id = message.thread_id.clone();
                let message_pubkey = message.pubkey.clone();

                // Mark tab as unread if it's not the active one
                app.mark_tab_unread(&thread_id);

                // Clear local streaming buffer when Nostr message arrives
                // This ensures streaming content is replaced by the final message
                app.clear_local_stream_buffer(&thread_id);

                // If this message is in the current thread...
                if app.selected_thread.as_ref().map(|t| t.id.as_str()) == Some(thread_id.as_str()) {
                    // Scroll to bottom
                    app.scroll_offset = usize::MAX;

                    // Sync agent selection if user hasn't explicitly picked one
                    // This ensures the input box reflects the agent who just responded
                    if !app.user_explicitly_selected_agent {
                        // Check if this message is from an agent (not the user)
                        let is_from_agent = app.data_store.borrow().user_pubkey
                            .as_ref()
                            .map(|pk| pk != &message_pubkey)
                            .unwrap_or(true);

                        if is_from_agent {
                            app.sync_agent_with_conversation();
                        }
                    }
                }
            }
            CoreEvent::ProjectStatus(status) => {
                if app.selected_project.as_ref().map(|p| p.a_tag()) == Some(status.project_coordinate.clone()) {
                    if app.selected_agent.is_none() {
                        if let Some(pm) = status.pm_agent() {
                            app.selected_agent = Some(pm.clone());
                        }
                    }
                    if app.selected_branch.is_none() {
                        app.selected_branch = status.default_branch().map(String::from);
                    }
                }
            }
        }
    }
}

/// Check if a pending new thread has arrived and navigate to it
fn check_pending_new_thread(app: &mut App) {
    let Some(project_a_tag) = app.pending_new_thread_project.clone() else {
        return;
    };

    let user_pubkey = app.keys.as_ref().map(|k| k.public_key().to_hex());
    let Some(user_pubkey) = user_pubkey else {
        return;
    };

    // Find the most recent thread from user (threads sorted by last_activity desc)
    let thread = {
        let store = app.data_store.borrow();
        store.get_threads(&project_a_tag)
            .iter()
            .find(|t| t.pubkey == user_pubkey)
            .cloned()
    };

    if let Some(thread) = thread {
        app.pending_new_thread_project = None;
        app.creating_thread = false;
        app.open_thread_from_home(&thread, &project_a_tag);
    }
}
