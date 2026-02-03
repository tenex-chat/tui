use crate::ui::App;

/// Export a thread as JSONL (one raw event per line)
pub(crate) fn export_thread_as_jsonl(app: &mut App, thread_id: &str) {
    use crate::store::get_raw_event_json;

    let messages = app.data_store.borrow().get_messages(thread_id).to_vec();

    if messages.is_empty() {
        app.set_warning_status("No messages to export");
        return;
    }

    let mut lines = Vec::new();

    // First, add the thread root event if available
    if let Some(json) = get_raw_event_json(&app.db.ndb, thread_id) {
        lines.push(json);
    }

    // Then add all message events
    for msg in &messages {
        if msg.id != thread_id {
            if let Some(json) = get_raw_event_json(&app.db.ndb, &msg.id) {
                lines.push(json);
            }
        }
    }

    let content = lines.join("\n");

    use arboard::Clipboard;
    match Clipboard::new() {
        Ok(mut clipboard) => {
            if clipboard.set_text(&content).is_ok() {
                app.set_warning_status(&format!(
                    "Exported {} events to clipboard as JSONL",
                    lines.len()
                ));
            } else {
                app.set_warning_status("Failed to copy to clipboard");
            }
        }
        Err(_) => {
            app.set_warning_status("Failed to access clipboard");
        }
    }
}
