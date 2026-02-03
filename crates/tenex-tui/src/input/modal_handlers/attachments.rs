use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::App;

pub(super) fn handle_attachment_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        KeyCode::Esc => app.cancel_attachment_modal(),
        KeyCode::Char('s') if has_ctrl => app.save_and_close_attachment_modal(),
        KeyCode::Char('d') if has_ctrl => app.delete_attachment_and_close_modal(),
        KeyCode::Enter => app.attachment_modal_editor_mut().insert_newline(),
        KeyCode::Char('a') if has_ctrl => app.attachment_modal_editor_mut().move_to_line_start(),
        KeyCode::Char('e') if has_ctrl => app.attachment_modal_editor_mut().move_to_line_end(),
        KeyCode::Char('k') if has_ctrl => app.attachment_modal_editor_mut().kill_to_line_end(),
        KeyCode::Left if has_alt => app.attachment_modal_editor_mut().move_word_left(),
        KeyCode::Right if has_alt => app.attachment_modal_editor_mut().move_word_right(),
        KeyCode::Left => app.attachment_modal_editor_mut().move_left(),
        KeyCode::Right => app.attachment_modal_editor_mut().move_right(),
        KeyCode::Backspace => app.attachment_modal_editor_mut().delete_char_before(),
        KeyCode::Delete => app.attachment_modal_editor_mut().delete_char_at(),
        KeyCode::Char(c) => app.attachment_modal_editor_mut().insert_char(c),
        _ => {}
    }
}

pub(super) fn handle_expanded_editor_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    match code {
        KeyCode::Esc => app.cancel_expanded_editor(),
        KeyCode::Char('s') if has_ctrl => app.save_and_close_expanded_editor(),
        KeyCode::Enter => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.insert_newline();
            }
        }
        KeyCode::Char('z') if has_ctrl && has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.redo();
            }
        }
        KeyCode::Char('z') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.undo();
            }
        }
        KeyCode::Char('a') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.select_all();
            }
        }
        KeyCode::Left if has_alt && has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_left_extend_selection();
            }
        }
        KeyCode::Right if has_alt && has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_right_extend_selection();
            }
        }
        KeyCode::Left if has_alt => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_left();
            }
        }
        KeyCode::Right if has_alt => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_right();
            }
        }
        KeyCode::Left if has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_left_extend_selection();
            }
        }
        KeyCode::Right if has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_right_extend_selection();
            }
        }
        KeyCode::Left => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_left();
            }
        }
        KeyCode::Right => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_right();
            }
        }
        KeyCode::Up => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_down();
            }
        }
        KeyCode::Backspace => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.delete_char_before();
            }
        }
        KeyCode::Delete => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.delete_char_at();
            }
        }
        KeyCode::Char('c') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                if let Some(selected) = editor.selected_text() {
                    use arboard::Clipboard;
                    if let Ok(mut clipboard) = Clipboard::new() {
                        let _ = clipboard.set_text(selected);
                    }
                }
            }
        }
        KeyCode::Char('x') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                if let Some(selected) = editor.selected_text() {
                    use arboard::Clipboard;
                    if let Ok(mut clipboard) = Clipboard::new() {
                        let _ = clipboard.set_text(selected);
                    }
                    editor.delete_selection();
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.insert_char(c);
            }
        }
        _ => {}
    }
}
