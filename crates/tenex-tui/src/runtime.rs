use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
    MouseEventKind,
};
use futures::StreamExt;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tenex_core::events::CoreEvent;
use tenex_core::runtime::CoreRuntime;

use crate::clipboard::{handle_clipboard_paste, handle_image_file_paste, UploadResult};
use crate::input::handle_key;
use crate::render::render;
use crate::ui::notifications::Notification;
use crate::ui::state::{TTSQueueItem, TTSQueueItemStatus};
use crate::ui::views::login::LoginStep;
use crate::ui::{App, InputMode, ModalState, Tui, View};

/// Result from background audio generation task
enum AudioGenerationResult {
    /// Audio generated successfully, contains path to audio file and source thread_id/message_id
    Success {
        audio_path: PathBuf,
        thread_id: String,
        message_id: String,
    },
    /// Audio generation failed or was skipped (not enabled, missing config, etc.)
    Skipped(String),
}

/// Result from background voice/model browse fetch tasks
pub(crate) enum BrowseResult {
    Voices(Result<Vec<tenex_core::ai::Voice>, String>),
    Models(Result<Vec<tenex_core::ai::Model>, String>),
}

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
    let (publish_confirm_tx, mut publish_confirm_rx) =
        tokio::sync::mpsc::channel::<(String, String)>(100);
    app.set_publish_confirm_tx(publish_confirm_tx);

    // Channel for receiving audio generation results from background tasks
    // When a p-tag mention triggers audio generation, the result is sent here
    let (audio_tx, mut audio_rx) = tokio::sync::mpsc::channel::<AudioGenerationResult>(10);

    // Channel for receiving voice/model browse results from background fetch tasks
    let (browse_tx, mut browse_rx) = tokio::sync::mpsc::channel::<BrowseResult>(4);
    app.browse_tx = Some(browse_tx);

    // BULLETPROOF: Surface draft storage load/parse errors at startup
    if let Some(error) = app.draft_storage_last_error() {
        log_diagnostic(&format!(
            "BULLETPROOF: Draft storage error at startup: {}",
            error
        ));
        app.set_warning_status(&format!("WARNING: Draft load error - {}", error));
        app.draft_storage_clear_error();
    }

    // BULLETPROOF: Clean up old confirmed publish snapshots on startup (>24h old)
    match app.cleanup_confirmed_publishes() {
        Ok(cleaned_up) if cleaned_up > 0 => {
            log_diagnostic(&format!(
                "BULLETPROOF: Cleaned up {} old confirmed publish snapshots",
                cleaned_up
            ));
        }
        Err(e) => {
            log_diagnostic(&format!(
                "BULLETPROOF: Error cleaning up publish snapshots: {}",
                e
            ));
        }
        _ => {}
    }

    // Check for pending (unconfirmed) publish snapshots on startup (recovery)
    let pending_publishes = app.get_pending_publishes();
    if !pending_publishes.is_empty() {
        log_diagnostic(&format!(
            "BULLETPROOF: Found {} pending (unconfirmed) publish snapshots that may need recovery",
            pending_publishes.len()
        ));
        // These are messages that were sent but never got relay confirmation
    }

    // Check for unpublished drafts on startup (recovery)
    let unpublished = app.get_unpublished_drafts();
    if !unpublished.is_empty() {
        log_diagnostic(&format!(
            "BULLETPROOF: Found {} drafts with content",
            unpublished.len()
        ));
    }

    // Set up a SIGTERM handler so that `kill <pid>` / supervisor shutdown triggers
    // a graceful exit rather than an immediate OS termination.  Graceful exit means
    // the `while app.running` loop exits normally, which lets `main()` call
    // `core_runtime.shutdown()` → `AppDataStore::save_cache()`.
    //
    // This is the primary reason the cache was never written: SIGTERM killed the
    // process before the shutdown sequence could run.
    //
    // Implementation note: we use a oneshot channel + spawned task rather than
    // `#[cfg(unix)]` inside `tokio::select!` because the select! macro does not
    // support attribute macros on its arms.  On non-Unix targets `_sigterm_guard`
    // keeps the sender alive so the receiver stays forever-pending.
    let (sigterm_tx, mut sigterm_rx) = tokio::sync::oneshot::channel::<()>();
    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sig) = signal(SignalKind::terminate()) {
            sig.recv().await;
            let _ = sigterm_tx.send(());
        }
    });
    #[cfg(not(unix))]
    let _sigterm_guard = sigterm_tx;

    let mut loop_count: u64 = 0;
    let mut terminal_events: u64 = 0;
    let mut ndb_events: u64 = 0;
    let mut tick_events: u64 = 0;
    let mut upload_events: u64 = 0;
    let mut publish_confirm_events: u64 = 0;
    let mut audio_events: u64 = 0;
    let diag_start = Instant::now();

    while app.running {
        loop_count += 1;

        // Log diagnostics every 1000 iterations
        if loop_count.is_multiple_of(1000) {
            let elapsed = diag_start.elapsed().as_secs();
            log_diagnostic(&format!(
                "loops={} elapsed={}s rate={}/s | terminal={} ndb={} tick={} upload={} publish_confirm={} audio={}",
                loop_count,
                elapsed,
                if elapsed > 0 { loop_count / elapsed } else { loop_count },
                terminal_events,
                ndb_events,
                tick_events,
                upload_events,
                publish_confirm_events,
                audio_events
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
                                // Ctrl+V - clipboard paste
                                app.pending_quit = false;
                                if app.view == View::Chat && app.input_mode == InputMode::Editing {
                                    // Chat view: check for image in clipboard first, then text
                                    if let Some(keys) = app.keys.clone() {
                                        handle_clipboard_paste(app, &keys, upload_tx.clone());
                                    }
                                } else {
                                    // Default: read clipboard text and dispatch as key events
                                    // so any input that handles KeyCode::Char gets paste for free
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        if let Ok(text) = clipboard.get_text() {
                                            dispatch_paste_as_keys(app, &text, login_step, pending_nsec)?;
                                        }
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
                            // Chat view with editing mode: smart paste (image detection, code formatting)
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
                            } else {
                                // Default: dispatch as key events through normal input handling.
                                // Any input that handles KeyCode::Char gets paste for free —
                                // no per-modal wiring needed.
                                dispatch_paste_as_keys(app, &text, login_step, pending_nsec)?;
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
                handle_core_events(app, events, audio_tx.clone());

                // Check for pending new thread and navigate to it if found
                check_pending_new_thread(app);
            }

            // Tick for regular updates (data channel polling for non-message updates)
            _ = tick_interval.tick() => {
                tick_events += 1;
                app.tick(); // Increment frame counter for animations
                app.check_for_data_updates()?;

                // Sync working agent state to open tabs (for blue indicator)
                sync_tab_working_state(app);

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
                        // Use insert_str for atomic insertion (single undo operation)
                        app.chat_editor_mut().insert_str(&marker);
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
                        if publish_confirm_events.is_multiple_of(10) {
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

            // Handle audio generation results from background tasks
            Some(result) = audio_rx.recv() => {
                audio_events += 1;
                match result {
                    AudioGenerationResult::Success { audio_path, thread_id, message_id } => {
                        log_diagnostic(&format!("AUDIO: Generated notification audio: {:?}", audio_path));

                        // Create a queue item for the TTS Control tab
                        let file_name = audio_path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("audio")
                            .to_string();
                        let mut queue_item = TTSQueueItem::new(&file_name);
                        queue_item.audio_path = Some(audio_path.clone());
                        queue_item.status = TTSQueueItemStatus::Playing;
                        // Set the source conversation and message for Enter navigation
                        queue_item.conversation_id = Some(thread_id);
                        queue_item.message_id = Some(message_id);

                        // Ensure TTS Control tab exists and add item to queue
                        let tts_tab_idx = app.tabs.open_tts_control();

                        // Add item to queue and set as playing
                        if let Some(tts_state) = app.tabs.tts_state_mut() {
                            // Mark any previous playing item as completed
                            if let Some(playing_idx) = tts_state.playing_index {
                                if let Some(item) = tts_state.queue.get_mut(playing_idx) {
                                    item.status = TTSQueueItemStatus::Completed;
                                }
                            }
                            // Add new item and set as playing
                            tts_state.queue.push(queue_item);
                            let new_idx = tts_state.queue.len() - 1;
                            tts_state.playing_index = Some(new_idx);
                            tts_state.selected_index = new_idx;
                        }

                        // Switch to TTS tab and Chat view if not already there
                        if app.view != View::Chat || app.tabs.active_index() != tts_tab_idx {
                            app.tabs.switch_to(tts_tab_idx);
                            app.view = View::Chat;
                            // Force Normal mode so TTS controls respond immediately
                            // (user may have been in Editing mode in a conversation)
                            app.input_mode = InputMode::Normal;
                        }

                        // Play the audio using the app's audio player
                        if let Err(e) = app.audio_player.play(&audio_path) {
                            log_diagnostic(&format!("AUDIO: Failed to play audio: {}", e));
                            // Mark the item as failed so tick_tts_playback doesn't mark it as Completed
                            if let Some(tts_state) = app.tabs.tts_state_mut() {
                                if let Some(playing_idx) = tts_state.playing_index {
                                    if let Some(item) = tts_state.queue.get_mut(playing_idx) {
                                        item.status = TTSQueueItemStatus::Failed;
                                    }
                                }
                                tts_state.playing_index = None;
                            }
                        }
                    }
                    AudioGenerationResult::Skipped(reason) => {
                        log_diagnostic(&format!("AUDIO: Skipped - {}", reason));
                    }
                }
            }

            // Handle voice/model browse results from background fetch tasks
            Some(result) = browse_rx.recv() => {
                use crate::ui::modal::{VoiceBrowseItem, ModelBrowseItem};

                match &mut app.modal_state {
                    ModalState::AppSettings(ref mut state) => {
                        match result {
                            BrowseResult::Voices(Ok(voices)) => {
                                if let Some(ref mut browser) = state.voice_browser {
                                    browser.loading = false;
                                    browser.items = voices.into_iter().map(|v| VoiceBrowseItem {
                                        voice_id: v.voice_id,
                                        name: v.name,
                                        category: v.category,
                                    }).collect();
                                }
                            }
                            BrowseResult::Voices(Err(e)) => {
                                state.voice_browser = None;
                                app.set_warning_status(&format!("Failed to fetch voices: {}", e));
                            }
                            BrowseResult::Models(Ok(models)) => {
                                if let Some(ref mut browser) = state.model_browser {
                                    browser.loading = false;
                                    browser.items = models.into_iter().map(|m| ModelBrowseItem {
                                        id: m.id,
                                        name: m.name,
                                        context_length: m.context_length,
                                    }).collect();
                                }
                            }
                            BrowseResult::Models(Err(e)) => {
                                state.model_browser = None;
                                app.set_warning_status(&format!("Failed to fetch models: {}", e));
                            }
                        }
                    }
                    _ => {
                        // Modal was closed before results arrived — discard
                    }
                }
            }

            // Gracefully handle SIGTERM (e.g. `kill <pid>`, supervisor shutdown).
            // Without this arm the OS terminates the process immediately, bypassing
            // `core_runtime.shutdown()` and therefore `AppDataStore::save_cache()`.
            Ok(()) = &mut sigterm_rx => {
                app.quit();
            }
        }
    }
    Ok(())
}

/// Sync the working agent state from data store to open tabs.
/// This updates the `is_agent_working` flag on each tab based on kind:24133 operation events.
fn sync_tab_working_state(app: &mut App) {
    let store = app.data_store.borrow();

    // Collect thread IDs and their working status
    let working_status: Vec<(String, bool)> = app
        .open_tabs()
        .iter()
        .filter(|tab| tab.is_conversation() && !tab.thread_id.is_empty())
        .map(|tab| {
            let is_working = !store
                .operations
                .get_working_agents(&tab.thread_id)
                .is_empty();
            (tab.thread_id.clone(), is_working)
        })
        .collect();

    drop(store); // Release borrow before mutating app

    // Update tab states
    for (thread_id, is_working) in working_status {
        app.set_tab_agent_working(&thread_id, is_working);
    }
}

fn dispatch_paste_as_keys(
    app: &mut App,
    text: &str,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    for c in text.chars() {
        if c == '\n' || c == '\r' {
            continue;
        }
        let key = KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        handle_key(app, key, login_step, pending_nsec)?;
    }
    Ok(())
}

fn handle_core_events(
    app: &mut App,
    events: Vec<CoreEvent>,
    audio_tx: tokio::sync::mpsc::Sender<AudioGenerationResult>,
) {
    for event in events {
        match event {
            CoreEvent::Message(message) => {
                let thread_id = message.thread_id.clone();
                let message_id = message.id.clone();
                let message_pubkey = message.pubkey.clone();
                let message_content = message.content.clone();
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

                        // Get thread title for notification (and audio generation)
                        let thread_title = app
                            .data_store
                            .borrow()
                            .get_thread_by_id(&thread_id)
                            .map(|t| t.title.clone())
                            .unwrap_or_else(|| "conversation".to_string());

                        // Push status bar notification if not viewing this thread
                        let is_viewing_thread = app.selected_thread().map(|t| t.id.as_str())
                            == Some(thread_id.as_str());

                        if !is_viewing_thread {
                            // Use message_for_user notification with thread_id for jump-to support
                            // Duration is 30 seconds and includes hint about Alt+M hotkey
                            let notification_msg =
                                format!("@ Message for you in {} · Alt+M to open", thread_title);
                            app.notify(Notification::message_for_user(
                                notification_msg,
                                thread_id.clone(),
                            ));
                        }

                        // Trigger audio notification generation (if enabled)
                        // This runs in a background task to avoid blocking the UI
                        trigger_audio_notification(
                            app,
                            audio_tx.clone(),
                            message_pubkey.clone(),
                            thread_title,
                            message_content,
                            thread_id.clone(),
                            message_id.clone(),
                        );
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
                        let is_from_agent = app
                            .data_store
                            .borrow()
                            .user_pubkey
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
                if app.selected_project.as_ref().map(|p| p.a_tag())
                    == Some(status.project_coordinate.clone())
                {
                    app.refresh_selected_agent_from_project_status();
                }
            }
            CoreEvent::PendingBackendApproval(pending) => {
                // Show approval modal if no modal is currently open
                if app.modal_state.is_none() {
                    app.show_backend_approval_modal(pending.backend_pubkey);
                }
            }
            CoreEvent::ReportUpsert(_report) => {
                // Reports are already stored by the core; no TUI-specific handling needed
            }
        }
    }
}

/// Trigger audio notification generation in a background task.
/// Checks if audio notifications are enabled and properly configured before spawning.
fn trigger_audio_notification(
    app: &App,
    audio_tx: tokio::sync::mpsc::Sender<AudioGenerationResult>,
    agent_pubkey: String,
    conversation_title: String,
    message_text: String,
    thread_id: String,
    message_id: String,
) {
    // Check if audio notifications are enabled in preferences
    let prefs = app.preferences.borrow();
    let ai_settings = &prefs.prefs.ai_audio_settings;

    if !ai_settings.enabled {
        // Audio notifications disabled - skip silently (no log spam)
        return;
    }

    // Check inactivity threshold: skip TTS if user was recently active in this thread
    let threshold = ai_settings.tts_inactivity_threshold_secs;
    if let Some(&last_activity) = app.last_user_activity_by_thread.get(&thread_id) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(last_activity) < threshold {
            log_diagnostic(&format!(
                "AUDIO: Skipping TTS for thread {} - user active {}s ago (threshold: {}s)",
                thread_id,
                now.saturating_sub(last_activity),
                threshold
            ));
            return;
        }
    }

    // Check if API keys are configured
    let elevenlabs_key = match prefs.get_elevenlabs_api_key() {
        Some(key) => key,
        None => {
            log_diagnostic("AUDIO: ElevenLabs API key not configured, skipping audio notification");
            return;
        }
    };

    let openrouter_key = match prefs.get_openrouter_api_key() {
        Some(key) => key,
        None => {
            log_diagnostic("AUDIO: OpenRouter API key not configured, skipping audio notification");
            return;
        }
    };

    // Check if voice IDs and model are configured
    if ai_settings.selected_voice_ids.is_empty() {
        log_diagnostic("AUDIO: No voices configured, skipping audio notification");
        return;
    }

    if ai_settings.openrouter_model.is_none() {
        log_diagnostic("AUDIO: OpenRouter model not configured, skipping audio notification");
        return;
    }

    // Clone settings for the background task
    let settings = ai_settings.clone();
    drop(prefs); // Release borrow before spawning

    // Get data directory for audio file storage
    let data_dir = tenex_core::config::CoreConfig::default_data_dir();
    let data_dir_str = data_dir.to_string_lossy().to_string();

    // Clone thread_id and message_id for the async closure
    let thread_id_for_result = thread_id.clone();
    let message_id_for_result = message_id;

    // Spawn background task to generate audio
    tokio::spawn(async move {
        use tenex_core::ai::AudioNotificationManager;

        let manager = AudioNotificationManager::new(&data_dir_str);

        // Initialize audio notifications directory
        if let Err(e) = manager.init() {
            let _ = audio_tx
                .send(AudioGenerationResult::Skipped(format!(
                    "Failed to init audio dir: {}",
                    e
                )))
                .await;
            return;
        }

        // Generate the audio notification
        match manager
            .generate_notification(
                &agent_pubkey,
                &conversation_title,
                &message_text,
                &elevenlabs_key,
                &openrouter_key,
                &settings,
            )
            .await
        {
            Ok(notification) => {
                let path = PathBuf::from(&notification.audio_file_path);
                let _ = audio_tx
                    .send(AudioGenerationResult::Success {
                        audio_path: path,
                        thread_id: thread_id_for_result,
                        message_id: message_id_for_result,
                    })
                    .await;
            }
            Err(e) => {
                let _ = audio_tx
                    .send(AudioGenerationResult::Skipped(format!(
                        "Generation failed: {}",
                        e
                    )))
                    .await;
            }
        }
    });
}

/// Check if there are pending backend approvals and show modal
fn check_pending_backend_approvals(app: &mut App) {
    // Only show approval modal if no modal is currently open
    if !app.modal_state.is_none() {
        return;
    }

    // Drain pending approvals from data store and show first one
    let pending = app
        .data_store
        .borrow_mut()
        .drain_pending_backend_approvals();
    if let Some(first) = pending.into_iter().next() {
        app.show_backend_approval_modal(first.backend_pubkey);
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
        store
            .get_threads(&project_a_tag)
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
