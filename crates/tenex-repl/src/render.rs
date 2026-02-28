use std::collections::HashSet;
use std::io::{Stdout, Write};
use crossterm::{cursor, execute, queue};
use crossterm::terminal::{self, ClearType};
use crate::{DIM, YELLOW, WHITE_BOLD, RED, RESET, BG_INPUT, BG_HIGHLIGHT};
use crate::editor::{LineEditor, AttachmentKind};
use crate::completion::CompletionMenu;
use crate::panels::{ConfigPanel, PanelMode, StatusBarNav, StatsTab, StatsPanel, DelegationEntry, Q_TAG_DELEGATION_DENYLIST};
use crate::state::ReplState;
use crate::util::{term_width, thread_display_name, format_runtime, PROMPT_PREFIX_WIDTH, HALF_BLOCK_LOWER, HALF_BLOCK_UPPER};
use tenex_core::runtime::CoreRuntime;
use tenex_core::models::Message;

// ─── Delegation Bar Data Collection ─────────────────────────────────────────

/// Scan messages for the latest todo_write call and return the first in-progress
/// or first pending todo item as (title, is_in_progress).
fn get_delegation_todo_summary(messages: &[Message]) -> Option<(String, bool)> {
    let mut last_todos: Option<Vec<serde_json::Value>> = None;

    for msg in messages {
        let (tool_name, params) = if let (Some(name), Some(args)) = (&msg.tool_name, &msg.tool_args)
        {
            match serde_json::from_str::<serde_json::Value>(args) {
                Ok(p) => (name.to_lowercase(), p),
                Err(_) => continue,
            }
        } else {
            continue;
        };

        let is_todo = matches!(
            tool_name.as_str(),
            "todo_write" | "todowrite" | "mcp__tenex__todo_write"
        );
        if !is_todo {
            continue;
        }

        let arr = params
            .get("todos")
            .or_else(|| params.get("items"))
            .and_then(|v| v.as_array().cloned());
        if let Some(items) = arr {
            last_todos = Some(items);
        }
    }

    let todos = last_todos?;

    for item in &todos {
        let status = item
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        if status == "in_progress" {
            let title = item
                .get("content")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !title.is_empty() {
                return Some((title.to_string(), true));
            }
        }
    }

    for item in &todos {
        let status = item
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        if status == "pending" {
            let title = item
                .get("content")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !title.is_empty() {
                return Some((title.to_string(), false));
            }
        }
    }

    None
}

/// Collect delegation entries for the current conversation.
fn collect_delegation_entries(state: &ReplState, runtime: &CoreRuntime) -> Vec<DelegationEntry> {
    let conv_id = match &state.current_conversation {
        Some(id) => id.clone(),
        None => return Vec::new(),
    };

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let mut entries: Vec<DelegationEntry> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    // 1. If current thread has a parent, add "← parent" entry
    let has_parent = store_ref
        .get_thread_by_id(&conv_id)
        .and_then(|t| t.parent_conversation_id.as_ref())
        .is_some()
        || store_ref.runtime_hierarchy.get_parent(&conv_id).is_some();

    if has_parent {
        let parent_id = store_ref
            .get_thread_by_id(&conv_id)
            .and_then(|t| t.parent_conversation_id.clone())
            .or_else(|| store_ref.runtime_hierarchy.get_parent(&conv_id).cloned())
            .unwrap_or_default();

        if !parent_id.is_empty() {
            entries.push(DelegationEntry {
                thread_id: parent_id,
                label: "← parent".to_string(),
                is_busy: false,
                is_parent: true,
                current_activity: None,
                todo_summary: None,
            });
        }
    }

    // 2. Collect child thread IDs from hierarchy + q_tags from messages
    let mut child_ids: Vec<String> = Vec::new();

    if let Some(children) = store_ref.runtime_hierarchy.get_children(&conv_id) {
        for child in children {
            child_ids.push(child.clone());
        }
    }

    let messages = store_ref.get_messages(&conv_id);
    for msg in messages {
        if !msg.q_tags.is_empty() {
            let tool = msg.tool_name.as_deref().unwrap_or("");
            if !Q_TAG_DELEGATION_DENYLIST.contains(&tool) {
                for q_tag in &msg.q_tags {
                    child_ids.push(q_tag.clone());
                }
            }
        }
    }

    // 3. Filter and build entries
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for child_id in &child_ids {
        if !seen_ids.insert(child_id.clone()) {
            continue;
        }

        let is_busy = store_ref.operations.is_event_busy(child_id);

        if !is_busy {
            if let Some(thread) = store_ref.get_thread_by_id(child_id) {
                if now_secs.saturating_sub(thread.effective_last_activity) > crate::util::DELEGATION_STALENESS_SECS {
                    continue;
                }
            } else {
                continue;
            }
        }

        let working = store_ref.operations.get_working_agents(child_id);
        let label = if !working.is_empty() {
            store_ref.get_profile_name(&working[0])
        } else if let Some(thread) = store_ref.get_thread_by_id(child_id) {
            thread_display_name(thread, 16)
        } else {
            format!("{}…", &child_id[..child_id.len().min(8)])
        };

        let current_activity = store_ref
            .get_thread_by_id(child_id)
            .and_then(|t| t.status_current_activity.clone());

        let child_messages = store_ref.get_messages(child_id);
        let todo_summary = get_delegation_todo_summary(child_messages);

        entries.push(DelegationEntry {
            thread_id: child_id.clone(),
            label,
            is_busy,
            is_parent: false,
            current_activity,
            todo_summary,
        });
    }

    entries
}

/// Update delegation bar state from the store.
pub(crate) fn update_delegation_bar(state: &mut ReplState, runtime: &CoreRuntime) {
    state.delegation_bar.visible_delegations = collect_delegation_entries(state, runtime);
    if state.delegation_bar.selected >= state.delegation_bar.visible_delegations.len() {
        state.delegation_bar.selected = 0;
    }
}

/// Render the delegation bar as a single line with BG_INPUT background.
/// Left: delegation chips, Right: todo/activity summary.
/// Returns the number of lines rendered (0 or 1).
fn render_delegation_bar(stdout: &mut Stdout, state: &ReplState, width: usize) -> u16 {
    if !state.delegation_bar.has_content() {
        return 0;
    }

    let bar = &state.delegation_bar;
    let mut left_parts: Vec<String> = Vec::new();
    let mut left_plain_width: usize = 2;

    for (i, entry) in bar.visible_delegations.iter().enumerate() {
        let is_selected = bar.focused && i == bar.selected;

        let (chip, chip_plain_len) = if entry.is_parent {
            if is_selected {
                (
                    format!("{BG_HIGHLIGHT}{WHITE_BOLD} ← parent {RESET}{BG_INPUT}"),
                    10,
                )
            } else {
                (format!("{DIM} ← parent {RESET}{BG_INPUT}"), 10)
            }
        } else {
            let indicator = if entry.is_busy { "⟡" } else { "○" };
            let label = &entry.label;
            if is_selected {
                (
                    format!(
                        " {indicator} {BG_HIGHLIGHT}{WHITE_BOLD}[▸{label}]{RESET}{BG_INPUT}"
                    ),
                    3 + 2 + label.chars().count() + 1,
                )
            } else {
                (
                    format!(" {indicator} [{label}]"),
                    3 + 1 + label.chars().count() + 1,
                )
            }
        };

        left_plain_width += chip_plain_len;
        left_parts.push(chip);
    }

    let right_entry = if bar.focused {
        bar.selected_entry()
    } else {
        bar.visible_delegations
            .iter()
            .find(|e| e.is_busy && !e.is_parent)
    };

    let (right_text, right_plain_width) = if let Some(entry) = right_entry {
        if let Some((ref title, is_in_progress)) = entry.todo_summary {
            let icon = if is_in_progress { "⊙" } else { "○" };
            let short_title = if title.chars().count() > 30 {
                let truncated: String = title.chars().take(29).collect();
                format!("{truncated}…")
            } else {
                title.clone()
            };
            let text = format!("{DIM}{icon} {short_title}{RESET}{BG_INPUT}");
            let plain = 2 + short_title.chars().count();
            (text, plain)
        } else if let Some(ref activity) = entry.current_activity {
            let short = if activity.chars().count() > 30 {
                let truncated: String = activity.chars().take(29).collect();
                format!("{truncated}…")
            } else {
                activity.clone()
            };
            let text = format!("{DIM}⊙ {short}{RESET}{BG_INPUT}");
            let plain = 2 + short.chars().count();
            (text, plain)
        } else {
            (String::new(), 0)
        }
    } else {
        (String::new(), 0)
    };

    let gap = width.saturating_sub(left_plain_width + right_plain_width + 2);
    write!(
        stdout,
        "\r\n{BG_INPUT}  {}{}{right_text}{}{RESET}",
        left_parts.join(""),
        " ".repeat(gap),
        " ".repeat(2),
    )
    .ok();

    1
}

// ─── Terminal Drawing ───────────────────────────────────────────────────────

/// Build the runtime indicator string with wave animation when agents are active.
/// Returns (ansi_colored_string, plain_char_count).
pub(crate) fn build_runtime_indicator(
    cumulative_runtime_ms: u64,
    has_active_agents: bool,
    active_agent_count: usize,
    wave_frame: u64,
) -> (String, usize) {
    let label = format!("Today: {} ", format_runtime(cumulative_runtime_ms));
    let plain_width = label.len();

    if !has_active_agents {
        return (format!("{RED}{label}{RESET}"), plain_width);
    }

    let base_r: f32 = 106.0;
    let base_g: f32 = 153.0;
    let base_b: f32 = 85.0;
    let wave_phase_speed: f32 = 0.3;
    let wave_wavelength: f32 = 0.8;
    let wave_period: f32 = 12.0;

    let agent_count_clamped = active_agent_count.clamp(1, 10) as f32;
    let speed_multiplier = 0.3 * agent_count_clamped;
    let brightness_amplitude = 0.3 + (0.3 * (agent_count_clamped - 1.0) / 9.0);
    let offset = (wave_frame / 2) as f32;

    let mut out = String::new();
    for (i, ch) in label.chars().enumerate() {
        let phase = ((offset * wave_phase_speed * speed_multiplier)
            + (i as f32 * wave_wavelength))
            * std::f32::consts::PI
            * 2.0
            / wave_period;
        let wave_value = phase.sin();
        let brightness = 1.0 + (wave_value * brightness_amplitude);
        let r = (base_r * brightness).clamp(0.0, 255.0) as u8;
        let g = (base_g * brightness).clamp(0.0, 255.0) as u8;
        let b = (base_b * brightness).clamp(0.0, 255.0) as u8;
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m{ch}"));
    }
    out.push_str(RESET);

    (out, plain_width)
}

/// Draw the full input area with half-block padding.
pub(crate) fn redraw_input(
    stdout: &mut Stdout,
    state: &ReplState,
    runtime: &CoreRuntime,
    editor: &LineEditor,
    completion: &mut CompletionMenu,
    panel: &ConfigPanel,
    status_nav: &StatusBarNav,
    stats_panel: &StatsPanel,
) {
    let width = term_width() as usize;

    clear_input_area(stdout, completion);

    // ── Top edge: lower half-blocks in input bg color ──
    let fg_input_bg = "\x1b[38;5;234m";
    write!(stdout, "{fg_input_bg}{}{RESET}", HALF_BLOCK_LOWER.to_string().repeat(width)).ok();

    // ── Delegation bar (0 or 1 lines between top edge and input) ──
    completion.delegation_bar_lines = render_delegation_bar(stdout, state, width);

    // ── Backend approval prompt (replaces normal input when pending) ──
    {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let pending_info = store_ref.trust.pending_backend_approvals.first().map(|a| {
            let name = store_ref.get_profile_name(&a.backend_pubkey);
            let short_pk = if a.backend_pubkey.len() >= 12 {
                a.backend_pubkey[..12].to_string()
            } else {
                a.backend_pubkey.clone()
            };
            (name, short_pk)
        });
        drop(store_ref);
        if let Some((name, short_pk)) = pending_info {
            let prompt = format!("  New backend: {YELLOW}{name}{RESET}{BG_INPUT}{DIM} ({short_pk}…){RESET}{BG_INPUT}  {WHITE_BOLD}[a]{RESET}{BG_INPUT}pprove  {WHITE_BOLD}[b]{RESET}{BG_INPUT}lock");
            let visible_len = 2 + "New backend: ".len() + name.len() + 2 + short_pk.len() + 2 + 2 + "[a]pprove  [b]lock".len();
            let prompt_pad = width.saturating_sub(visible_len);
            write!(stdout, "\r\n{BG_INPUT}{prompt}{}{RESET}", " ".repeat(prompt_pad)).ok();
            completion.input_wrap_lines = 0;
            completion.attachment_indicator_lines = 0;
            completion.rendered_lines = 0;

            write!(stdout, "\r\n{fg_input_bg}{}{RESET}", HALF_BLOCK_UPPER.to_string().repeat(width)).ok();
            let (status_text, status_plain_width) = state.status_bar_text(runtime);
            let (rt_ms, rt_active, rt_count) = {
                let store = runtime.data_store();
                let store_ref = store.borrow();
                store_ref.get_statusbar_runtime_ms()
            };
            let (rt_ansi, rt_plain_width) =
                build_runtime_indicator(rt_ms, rt_active, rt_count, state.wave_frame);
            let left_used = 3 + status_plain_width;
            let gap = width.saturating_sub(left_used + rt_plain_width);
            write!(
                stdout,
                "\r\n{DIM}   {status_text}{RESET}{}{rt_ansi}",
                " ".repeat(gap)
            )
            .ok();

            queue!(stdout, cursor::MoveUp(2)).ok();
            queue!(stdout, cursor::MoveToColumn(visible_len as u16)).ok();
            stdout.flush().ok();
            completion.cursor_row = 0;
            completion.input_area_drawn = true;
            return;
        }
    }

    // ── Input line (dark background, full width) ──
    let display_buffer = if panel.active { &panel.origin_command } else { &editor.buffer };
    let input_visible_len = PROMPT_PREFIX_WIDTH as usize + display_buffer.chars().count();
    let input_total_rows = if width > 0 { input_visible_len.saturating_sub(1) / width + 1 } else { 1 };
    let last_row_chars = if width > 0 { ((input_visible_len.saturating_sub(1)) % width) + 1 } else { input_visible_len };
    let pad = width.saturating_sub(last_row_chars);
    write!(stdout, "\r\n{BG_INPUT}{WHITE_BOLD}  › {RESET}{BG_INPUT}{}{}{RESET}",
        display_buffer,
        " ".repeat(pad),
    ).ok();
    completion.input_wrap_lines = (input_total_rows as u16).saturating_sub(1);

    // ── Attachment strip (below input, if attachments exist) ──
    if editor.has_attachments() {
        write!(stdout, "\r\n{BG_INPUT}  📎 ").ok();
        let mut strip_chars: usize = 5;
        for (i, att) in editor.attachments.iter().enumerate() {
            let label = match &att.kind {
                AttachmentKind::Text { content } => {
                    let lines = content.lines().count();
                    format!("Text Att. {} ({} line{})", att.id, lines, if lines == 1 { "" } else { "s" })
                }
                AttachmentKind::Image { .. } => format!("Image #{}", att.id),
            };
            let is_selected = editor.selected_attachment == Some(i);
            if is_selected {
                write!(stdout, " {WHITE_BOLD}{BG_HIGHLIGHT} {label} {RESET}{BG_INPUT}").ok();
            } else {
                write!(stdout, " {DIM} {label} {RESET}{BG_INPUT}").ok();
            }
            strip_chars += 2 + label.len() + 1;
        }
        let strip_pad = width.saturating_sub(strip_chars);
        write!(stdout, "{}{RESET}", " ".repeat(strip_pad)).ok();
        completion.attachment_indicator_lines = 1;
    } else {
        completion.attachment_indicator_lines = 0;
    }

    // ── Config panel or completion menu ──
    if panel.active {
        let header = match panel.mode {
            PanelMode::Tools => format!("  Tools for {}", panel.agent_name),
            PanelMode::Model => format!("  Model for {}", panel.agent_name),
        };
        let header_len = header.chars().count();
        let header_pad = width.saturating_sub(header_len);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(stdout, "{BG_INPUT}{WHITE_BOLD}{header}{}{RESET}", " ".repeat(header_pad)).ok();

        let max_visible = 15;
        let visible_end = (panel.scroll_offset + max_visible).min(panel.items.len());
        let visible_count = visible_end - panel.scroll_offset;
        for i in panel.scroll_offset..visible_end {
            let item = &panel.items[i];
            let (marker, marker_len) = match panel.mode {
                PanelMode::Tools => {
                    if panel.selected.contains(item) {
                        ("[x] ", 4)
                    } else {
                        ("[ ] ", 4)
                    }
                }
                PanelMode::Model => {
                    if panel.selected.contains(item) {
                        ("(*) ", 4)
                    } else {
                        ("( ) ", 4)
                    }
                }
            };
            let text = format!("  {marker}{item}");
            let text_len = 2 + marker_len + item.chars().count();
            let item_pad = width.saturating_sub(text_len);
            write!(stdout, "\r\n").ok();
            queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
            if i == panel.cursor {
                write!(stdout, "{BG_HIGHLIGHT}{WHITE_BOLD}{text}{}{RESET}", " ".repeat(item_pad)).ok();
            } else {
                write!(stdout, "{BG_INPUT}{DIM}{text}{}{RESET}", " ".repeat(item_pad)).ok();
            }
        }

        let footer = match panel.mode {
            PanelMode::Tools => "  Space: toggle  Enter: save  Esc: cancel",
            PanelMode::Model => "  Space/Enter: select  Esc: cancel",
        };
        let footer_len = footer.chars().count();
        let footer_pad = width.saturating_sub(footer_len);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(stdout, "{BG_INPUT}{DIM}{footer}{}{RESET}", " ".repeat(footer_pad)).ok();

        completion.rendered_lines = (1 + visible_count + 1) as u16;
    } else if stats_panel.active {
        let tabs = [StatsTab::Rankings, StatsTab::Runtime, StatsTab::Messages];
        let mut tab_bar = String::from("  ");
        let mut tab_bar_plain_len = 2;
        for (i, tab) in tabs.iter().enumerate() {
            let label = tab.label();
            if i > 0 {
                tab_bar.push_str("  ");
                tab_bar_plain_len += 2;
            }
            if *tab == stats_panel.tab {
                tab_bar.push_str(&format!("{WHITE_BOLD}╸{label}╺{RESET}"));
            } else {
                tab_bar.push_str(&format!("{DIM} {label} {RESET}"));
            }
            tab_bar_plain_len += label.len() + 2;
        }
        let tab_bar_pad = width.saturating_sub(tab_bar_plain_len);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(stdout, "{BG_INPUT}{tab_bar}{}{RESET}", " ".repeat(tab_bar_pad)).ok();

        let sep = format!("  {}", "─".repeat(width.saturating_sub(4)));
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(stdout, "{BG_INPUT}{DIM}{sep}{RESET}").ok();

        let max_visible = 16;
        let visible_end = (stats_panel.scroll_offset + max_visible).min(stats_panel.total_lines);
        let visible_count = visible_end.saturating_sub(stats_panel.scroll_offset);
        for i in stats_panel.scroll_offset..visible_end {
            let line = &stats_panel.content_lines[i];
            let pw = stats_panel.content_plain_widths[i];
            let line_pad = width.saturating_sub(pw);
            write!(stdout, "\r\n").ok();
            queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
            write!(stdout, "{BG_INPUT}{line}{}{RESET}", " ".repeat(line_pad)).ok();
        }

        let footer = "  Tab/←→: switch tab  ↑↓: scroll  Esc: close";
        let footer_len = footer.len();
        let footer_pad = width.saturating_sub(footer_len);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(stdout, "{BG_INPUT}{DIM}{footer}{}{RESET}", " ".repeat(footer_pad)).ok();

        completion.rendered_lines = (2 + visible_count + 1) as u16;
    } else if completion.visible && !completion.items.is_empty() {
        let count = completion.items.len().min(12) as u16;
        for (i, item) in completion.items.iter().take(12).enumerate() {
            let label = &item.label;
            let desc = &item.description;
            write!(stdout, "\r\n").ok();
            queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
            let text = format!("  {label:<24} {desc}");
            let text_len = 2 + label.chars().count().max(24) + 1 + desc.chars().count();
            let item_pad = width.saturating_sub(text_len);
            if i == completion.selected {
                write!(stdout, "{BG_INPUT}{WHITE_BOLD}{text}{}{RESET}", " ".repeat(item_pad)).ok();
            } else {
                write!(stdout, "{BG_INPUT}{DIM}{text}{}{RESET}", " ".repeat(item_pad)).ok();
            }
        }
        completion.rendered_lines = count;
    } else {
        completion.rendered_lines = 0;
    }

    // ── Bottom edge: upper half-blocks in input bg color ──
    let fg_input_bg = "\x1b[38;5;234m";
    write!(stdout, "\r\n{fg_input_bg}{}{RESET}", HALF_BLOCK_UPPER.to_string().repeat(width)).ok();

    // ── Status bar (project/agent on left, runtime on right) ──
    let (runtime_ms, has_active, active_count) = {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        store_ref.get_statusbar_runtime_ms()
    };
    if status_nav.active {
        let segments = state.status_bar_segments(runtime);
        let last_seg = segments.len().saturating_sub(1);
        let is_runtime_focused = status_nav.segment == last_seg;

        let (nav_text, nav_plain_width) = state.status_bar_text_navigable(runtime, status_nav.segment);
        let (runtime_ansi, runtime_plain_width) = if is_runtime_focused {
            let label = format!("Today: {} ", format_runtime(runtime_ms));
            let pw = label.len();
            (format!("\x1b[7m{label}\x1b[27m"), pw)
        } else {
            build_runtime_indicator(runtime_ms, has_active, active_count, state.wave_frame)
        };
        let left_used = 3 + nav_plain_width;
        let gap = width.saturating_sub(left_used + runtime_plain_width);
        write!(
            stdout,
            "\r\n   {nav_text}{RESET}{}{runtime_ansi}",
            " ".repeat(gap)
        )
        .ok();
    } else {
        let (status_text, status_plain_width) = state.status_bar_text(runtime);
        let (runtime_ansi, runtime_plain_width) =
            build_runtime_indicator(runtime_ms, has_active, active_count, state.wave_frame);
        let left_used = 3 + status_plain_width;
        let gap = width.saturating_sub(left_used + runtime_plain_width);
        write!(
            stdout,
            "\r\n{DIM}   {status_text}{RESET}{}{runtime_ansi}",
            " ".repeat(gap)
        )
        .ok();
    }

    // ── Position cursor back on input line ──
    let lines_below = completion.attachment_indicator_lines + completion.rendered_lines + 2;
    let cursor_char_pos = PROMPT_PREFIX_WIDTH as usize + editor.buffer[..editor.cursor].chars().count();
    let cursor_row = if width > 0 && cursor_char_pos > 0 { (cursor_char_pos - 1) / width } else { 0 };
    let wrap_lines_after_cursor = (completion.input_wrap_lines as usize).saturating_sub(cursor_row);
    queue!(stdout, cursor::MoveUp(lines_below + wrap_lines_after_cursor as u16)).ok();
    let col = if width > 0 && cursor_char_pos > 0 { ((cursor_char_pos - 1) % width) + 1 } else { 0 };
    queue!(stdout, cursor::MoveToColumn(col as u16)).ok();
    stdout.flush().ok();

    completion.cursor_row = cursor_row as u16;
    completion.input_area_drawn = true;
}

/// Clear the entire input area (top edge + input + completion + bottom edge + status bar).
/// After clearing, cursor is at the position where top edge should be drawn.
pub(crate) fn clear_input_area(stdout: &mut Stdout, completion: &mut CompletionMenu) {
    if !completion.input_area_drawn {
        return;
    }

    let up_to_top = completion.cursor_row + 1 + completion.delegation_bar_lines;
    queue!(stdout, cursor::MoveUp(up_to_top), cursor::MoveToColumn(0)).ok();
    queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();

    let lines_below = completion.delegation_bar_lines + 1 + completion.input_wrap_lines + completion.attachment_indicator_lines + completion.rendered_lines + 2;
    for _ in 0..lines_below {
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
    }
    queue!(stdout, cursor::MoveUp(lines_below)).ok();
    stdout.flush().ok();
    completion.rendered_lines = 0;
    completion.attachment_indicator_lines = 0;
    completion.delegation_bar_lines = 0;
    completion.input_wrap_lines = 0;
}

/// Clear the entire terminal, print lines near the bottom, update delegation bar, redraw input.
/// This is the single implementation of the ClearScreen pattern used when opening conversations,
/// navigating delegations, switching projects, etc.
pub(crate) fn apply_clear_screen(
    stdout: &mut Stdout,
    lines: &[String],
    state: &mut ReplState,
    runtime: &CoreRuntime,
    editor: &LineEditor,
    completion: &mut CompletionMenu,
    panel: &ConfigPanel,
    status_nav: &StatusBarNav,
    stats_panel: &StatsPanel,
) {
    execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
    completion.input_area_drawn = false;
    let (_, rows) = terminal::size().unwrap_or((80, 24));
    let content_rows = lines.len() as u16;
    let start = rows.saturating_sub(5 + content_rows);
    execute!(stdout, cursor::MoveTo(0, start)).ok();
    for l in lines {
        for part in l.split('\n') {
            write!(stdout, "{}\r\n", part).ok();
        }
    }
    stdout.flush().ok();
    update_delegation_bar(state, runtime);
    redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel);
}

/// Print text above the input area, then redraw the input area.
pub(crate) fn print_above_input(
    stdout: &mut Stdout,
    text: &str,
    state: &ReplState,
    runtime: &CoreRuntime,
    editor: &LineEditor,
    completion: &mut CompletionMenu,
    panel: &ConfigPanel,
    status_nav: &StatusBarNav,
    stats_panel: &StatsPanel,
) {
    clear_input_area(stdout, completion);
    completion.input_area_drawn = false;

    for line in text.split('\n') {
        write!(stdout, "{}\r\n", line).ok();
    }
    stdout.flush().ok();

    redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel);
}
