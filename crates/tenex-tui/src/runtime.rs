use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use futures::StreamExt;
use std::io::Write;
use std::time::{Duration, Instant};

use tenex_core::events::CoreEvent;
use tenex_core::runtime::CoreRuntime;

use crate::clipboard::{handle_clipboard_paste, handle_image_file_paste, UploadResult};
use crate::input::handle_key;
use crate::render::render;
use crate::ui::views::login::LoginStep;
use crate::ui::{App, InputMode, ModalState, Tui, View};
use crate::ui::notifications::Notification;

fn log_diagnostic(msg: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/tenex-diag.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

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

    // BULLETPROOF: Channel for receiving publish confirmations from worker threads
    // When a message is confirmed published to relay, we mark the draft as published
    let (publish_confirm_tx, mut publish_confirm_rx) = tokio::sync::mpsc::channel::<(String, String)>(100);
    app.set_publish_confirm_tx(publish_confirm_tx);

    // BULLETPROOF: Surface draft storage load/parse errors at startup
    if let Some(error) = app.draft_storage_last_error() {
        log_diagnostic(&format!("BULLETPROOF: Draft storage error at startup: {}", error));
        app.set_warning_status(&format!("WARNING: Draft load error - {}", error));
        app.draft_storage_clear_error();
    }

    // BULLETPROOF: Clean up old confirmed publish snapshots on startup (>24h old)
    match app.cleanup_confirmed_publishes() {
        Ok(cleaned_up) if cleaned_up > 0 => {
            log_diagnostic(&format!("BULLETPROOF: Cleaned up {} old confirmed publish snapshots", cleaned_up));
        }
        Err(e) => {
            log_diagnostic(&format!("BULLETPROOF: Error cleaning up publish snapshots: {}", e));
        }
        _ => {}
    }

    // Check for pending (unconfirmed) publish snapshots on startup (recovery)
    let pending_publishes = app.get_pending_publishes();
    if !pending_publishes.is_empty() {
        log_diagnostic(&format!("BULLETPROOF: Found {} pending (unconfirmed) publish snapshots that may need recovery", pending_publishes.len()));
        // These are messages that were sent but never got relay confirmation
    }

    // Check for unpublished drafts on startup (recovery)
    let unpublished = app.get_unpublished_drafts();
    if !unpublished.is_empty() {
        log_diagnostic(&format!("BULLETPROOF: Found {} drafts with content", unpublished.len()));
    }

    let mut loop_count: u64 = 0;
    let mut terminal_events: u64 = 0;
    let mut ndb_events: u64 = 0;
    let mut tick_events: u64 = 0;
    let mut upload_events: u64 = 0;
    let mut publish_confirm_events: u64 = 0;
    let diag_start = Instant::now();

    while app.running {
        loop_count += 1;

        // Log diagnostics every 1000 iterations
        if loop_count % 1000 == 0 {
            let elapsed = diag_start.elapsed().as_secs();
            log_diagnostic(&format!(
                "loops={} elapsed={}s rate={}/s | terminal={} ndb={} tick={} upload={} publish_confirm={}",
                loop_count,
                elapsed,
                if elapsed > 0 { loop_count / elapsed } else { loop_count },
                terminal_events,
                ndb_events,
                tick_events,
                upload_events,
                publish_confirm_events
            ));
        }

        // Render
        terminal.draw(|f| render(f, app, login_step))?;

        // Wait for events using tokio::select!
        tokio::select! {
            // Terminal UI events
            maybe_event = event_stream.next() => {
                terminal_events += 1;
                // Received terminal event
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
                                // Ctrl+V - check clipboard for image or pass through to modal
                                app.pending_quit = false;
                                // Check if debug stats modal on e-tag query tab
                                let is_debug_etag_input = matches!(
                                    &app.modal_state,
                                    ModalState::DebugStats(state) if state.active_tab == crate::ui::modal::DebugStatsTab::ETagQuery
                                );
                                if is_debug_etag_input {
                                    // Let modal handler process the paste
                                    handle_key(app, key, login_step, pending_nsec)?;
                                } else if app.view == View::Chat && app.input_mode == InputMode::Editing {
                                    if let Some(keys) = app.keys.clone() {
                                        handle_clipboard_paste(app, &keys, upload_tx.clone());
                                    }
                                }
                            } else {
                                // Any other key clears pending quit state
                                app.pending_quit = false;
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
                                            app.chat_editor_mut().handle_paste(&text);
                                            app.save_chat_draft();
                                        }
                                    } else {
                                        app.chat_editor_mut().handle_paste(&text);
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
                ndb_events += 1;
                let events = core_runtime.process_note_keys(&note_keys)?;
                handle_core_events(app, events);

                // Check for pending new thread and navigate to it if found
                check_pending_new_thread(app);
            }

            // Tick for regular updates (data channel polling for non-message updates)
            _ = tick_interval.tick() => {
                tick_events += 1;
                app.tick(); // Increment frame counter for animations
                app.check_for_data_updates()?;

                // Check for pending backend approvals
                check_pending_backend_approvals(app);
            }

            // Handle upload results from background tasks
            Some(result) = upload_rx.recv() => {
                upload_events += 1;
                match result {
                    UploadResult::Success(url) => {
                        let id = app.chat_editor_mut().add_image_attachment(url);
                        let marker = format!("[Image #{}] ", id);
                        for c in marker.chars() {
                            app.chat_editor_mut().insert_char(c);
                        }
                        app.save_chat_draft();
                        app.dismiss_notification();
                    }
                    UploadResult::Error(msg) => {
                        app.set_warning_status(&msg);
                    }
                }
            }

            // BULLETPROOF: Handle publish confirmations from worker threads
            // When relay confirms message was published, mark the specific snapshot as confirmed
            Some((publish_id, event_id)) = publish_confirm_rx.recv() => {
                publish_confirm_events += 1;
                match app.mark_publish_confirmed(&publish_id, Some(event_id.clone())) {
                    Ok(true) => {
                        log_diagnostic(&format!("BULLETPROOF: Publish snapshot '{}' confirmed (event_id={})",
                            &publish_id[..publish_id.len().min(16)],
                            &event_id[..event_id.len().min(12)]
                        ));
                        // BULLETPROOF: Periodically cleanup old confirmed snapshots to prevent accumulation
                        // Run after every 10 confirmations to balance I/O vs. memory
                        if publish_confirm_events % 10 == 0 {
                            match app.cleanup_confirmed_publishes() {
                                Ok(cleaned) if cleaned > 0 => {
                                    log_diagnostic(&format!("BULLETPROOF: Cleaned up {} old confirmed snapshots", cleaned));
                                }
                                Err(e) => {
                                    log_diagnostic(&format!("BULLETPROOF: Error cleaning up snapshots: {}", e));
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(false) => {
                        log_diagnostic(&format!("BULLETPROOF: Warning - publish snapshot '{}' not found for confirmation",
                            &publish_id[..publish_id.len().min(16)]
                        ));
                    }
                    Err(e) => {
                        log_diagnostic(&format!("BULLETPROOF: Error confirming publish snapshot '{}': {}",
                            &publish_id[..publish_id.len().min(16)], e
                        ));
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
                let p_tags = message.p_tags.clone();

                // Mark tab as unread if it's not the active one
                app.mark_tab_unread(&thread_id);

                // Check if this message p-tags the current user (waiting for response)
                // Exception: Self p-tagging (user's own messages) should NOT trigger this
                let user_pubkey = app.data_store.borrow().user_pubkey.clone();
                if let Some(ref pk) = user_pubkey {
                    let is_from_user = &message_pubkey == pk;
                    let ptags_user = p_tags.iter().any(|p| p == pk);

                    if !is_from_user && ptags_user {
                        // Mark as waiting for user (tab indicator)
                        app.mark_tab_waiting_for_user(&thread_id);

                        // Push status bar notification if not viewing this thread
                        let is_viewing_thread = app.selected_thread()
                            .map(|t| t.id.as_str()) == Some(thread_id.as_str());

                        if !is_viewing_thread {
                            // Get thread title for notification
                            let thread_title = app.data_store.borrow()
                                .get_thread_by_id(&thread_id)
                                .map(|t| t.title.clone())
                                .unwrap_or_else(|| "conversation".to_string());

                            // Use message_for_user notification with thread_id for jump-to support
                            // Duration is 30 seconds and includes hint about Alt+M hotkey
                            let notification_msg = format!("@ Message for you in {} · Alt+M to open", thread_title);
                            app.notify(Notification::message_for_user(notification_msg, thread_id.clone()));
                        }
                    }
                }

                // Clear local streaming buffer when Nostr message arrives
                // This ensures streaming content is replaced by the final message
                app.clear_local_stream_buffer(&thread_id);

                // If this message is in the current thread...
                if app.selected_thread().map(|t| t.id.as_str()) == Some(thread_id.as_str()) {
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

                    // Update sidebar state with delegations and reports from messages
                    // (done here on message arrival rather than during render for purity)
                    let messages = app.messages();
                    app.update_sidebar_from_messages(&messages);

                    // Check if this new message created a pending ask for the current thread
                    // and auto-open the modal (event-driven, not per-frame)
                    app.maybe_open_pending_ask();
                }
            }
            CoreEvent::ProjectStatus(status) => {
                if app.selected_project.as_ref().map(|p| p.a_tag()) == Some(status.project_coordinate.clone()) {
                    if app.selected_agent().is_none() {
                        if let Some(pm) = status.pm_agent() {
                            app.set_selected_agent(Some(pm.clone()));
                        }
                    }
                }
            }
            CoreEvent::PendingBackendApproval(pending) => {
                // Show approval modal if no modal is currently open
                if app.modal_state.is_none() {
                    app.show_backend_approval_modal(pending.backend_pubkey, pending.project_a_tag);
                }
            }
        }
    }
}

/// Check if there are pending backend approvals and show modal
fn check_pending_backend_approvals(app: &mut App) {
    // Only show approval modal if no modal is currently open
    if !app.modal_state.is_none() {
        return;
    }

    // Drain pending approvals from data store and show first one
    let pending = app.data_store.borrow_mut().drain_pending_backend_approvals();
    if let Some(first) = pending.into_iter().next() {
        app.show_backend_approval_modal(first.backend_pubkey, first.project_a_tag);
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
        // If we have a draft_id, convert the draft tab to a real tab
        if let Some(draft_id) = app.pending_new_thread_draft_id.take() {
            app.convert_draft_to_tab(&draft_id, &thread);
        }

        app.pending_new_thread_project = None;
        app.creating_thread = false;

        // Update selected_thread and open the tab
        app.set_selected_thread(Some(thread.clone()));
        app.open_tab(&thread, &project_a_tag);
        app.scroll_offset = usize::MAX; // Scroll to bottom
        app.input_mode = InputMode::Editing;
    }
}
