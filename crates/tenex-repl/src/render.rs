use crate::completion::CompletionMenu;
use crate::editor::{AttachmentKind, LineEditor};
use crate::panels::{
    ConfigPanel, DelegationEntry, NudgeSkillPanel, PanelMode, StatsPanel, StatsTab, StatusBarNav,
    Q_TAG_DELEGATION_DENYLIST,
};
use crate::state::ReplState;
use crate::util::{
    format_runtime, term_width, thread_display_name, HALF_BLOCK_LOWER, HALF_BLOCK_UPPER,
    PROMPT_PREFIX_WIDTH,
};
use crate::{ACCENT, BG_HIGHLIGHT, BG_INPUT, DIM, GREEN, RED, RESET, WHITE_BOLD};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use std::collections::{HashSet, VecDeque};
use std::io::{Stdout, Write};
use tenex_core::models::{AskQuestion, InputMode as AskInputMode, Message};
use tenex_core::runtime::CoreRuntime;

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
    let collect_direct_children = |thread_id: &str| -> Vec<String> {
        let mut children: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        if let Some(runtime_children) = store_ref.runtime_hierarchy.get_children(thread_id) {
            for child_id in runtime_children {
                if child_id != thread_id && !child_id.is_empty() && seen.insert(child_id.clone()) {
                    children.push(child_id.clone());
                }
            }
        }

        let messages = store_ref.get_messages(thread_id);
        for msg in messages {
            if msg.q_tags.is_empty() {
                continue;
            }
            let tool = msg.tool_name.as_deref().unwrap_or("");
            if Q_TAG_DELEGATION_DENYLIST.contains(&tool) {
                continue;
            }
            for q_tag in &msg.q_tags {
                if q_tag != thread_id && !q_tag.is_empty() && seen.insert(q_tag.clone()) {
                    children.push(q_tag.clone());
                }
            }
        }

        children.sort_by(|a, b| {
            let a_activity = store_ref
                .get_thread_by_id(a)
                .map(|t| t.effective_last_activity)
                .unwrap_or(0);
            let b_activity = store_ref
                .get_thread_by_id(b)
                .map(|t| t.effective_last_activity)
                .unwrap_or(0);
            b_activity.cmp(&a_activity).then_with(|| a.cmp(b))
        });
        children
    };

    // Traverse the delegation tree (children + grandchildren + deeper descendants).
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    for child_id in collect_direct_children(&conv_id) {
        queue.push_back((child_id, 1));
    }

    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut discovered: Vec<(String, usize)> = Vec::new();
    while let Some((thread_id, depth)) = queue.pop_front() {
        if !seen_ids.insert(thread_id.clone()) {
            continue;
        }
        discovered.push((thread_id.clone(), depth));
        for child_id in collect_direct_children(&thread_id) {
            if !seen_ids.contains(&child_id) {
                queue.push_back((child_id, depth + 1));
            }
        }
    }

    // Keep only unfinished delegations and build entries.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for (child_id, depth) in discovered {
        let is_busy = store_ref.operations.is_event_busy(&child_id);
        let thread = store_ref.get_thread_by_id(&child_id);
        let is_recent = thread
            .map(|t| {
                now_secs.saturating_sub(t.effective_last_activity)
                    <= crate::util::DELEGATION_STALENESS_SECS
            })
            .unwrap_or(false);

        let current_activity = thread.and_then(|t| t.status_current_activity.clone());
        let has_activity = current_activity
            .as_ref()
            .map(|a| !a.trim().is_empty())
            .unwrap_or(false);

        let child_messages = store_ref.get_messages(&child_id);
        let todo_summary = get_delegation_todo_summary(child_messages);

        // "Unfinished" means actively busy, has an unfinished todo item, or has fresh activity.
        if !(is_busy || todo_summary.is_some() || (has_activity && is_recent)) {
            continue;
        }

        let working = store_ref.operations.get_working_agents(&child_id);
        let label = if !working.is_empty() {
            store_ref.get_profile_name(&working[0])
        } else if let Some(thread) = thread {
            thread_display_name(thread, 16)
        } else {
            format!("{}…", &child_id[..child_id.len().min(8)])
        };

        entries.push(DelegationEntry {
            thread_id: child_id,
            depth,
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

const BG_DELEGATION_CARD: &str = "\x1b[48;5;236m";

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total = value.chars().count();
    if total <= max_chars {
        return value.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out: String = value.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

fn delegation_tree_prefix(depth: usize) -> String {
    if depth <= 1 {
        "↳".to_string()
    } else {
        format!("{}↳", "  ".repeat(depth.saturating_sub(1)))
    }
}

fn delegation_summary(entry: &DelegationEntry) -> Option<String> {
    if let Some((title, is_in_progress)) = &entry.todo_summary {
        let icon = if *is_in_progress { "⊙" } else { "○" };
        return Some(format!("{icon} {}", truncate_chars(title, 36)));
    }
    entry
        .current_activity
        .as_ref()
        .map(|activity| activity.trim())
        .filter(|activity| !activity.is_empty())
        .map(|activity| format!("⊙ {}", truncate_chars(activity, 36)))
}

fn wrapped_rows(visible_len: usize, width: usize) -> usize {
    if width == 0 {
        1
    } else {
        visible_len.saturating_sub(1) / width + 1
    }
}

fn input_total_rows(display_buffer: &str, prompt_width: usize, width: usize) -> usize {
    display_buffer
        .split('\n')
        .map(|line| wrapped_rows(prompt_width + line.chars().count(), width))
        .sum::<usize>()
        .max(1)
}

fn input_cursor_position(
    display_buffer: &str,
    cursor: usize,
    prompt_width: usize,
    width: usize,
) -> (usize, usize) {
    let clamped_cursor = cursor.min(display_buffer.len());
    let before_cursor = &display_buffer[..clamped_cursor];
    let before_lines: Vec<&str> = before_cursor.split('\n').collect();
    let current_line_idx = before_lines.len().saturating_sub(1);
    let col_chars = before_lines
        .last()
        .map(|line| line.chars().count())
        .unwrap_or(0);

    let all_lines: Vec<&str> = display_buffer.split('\n').collect();
    let mut row = 0usize;
    for line in all_lines.iter().take(current_line_idx) {
        row += wrapped_rows(prompt_width + line.chars().count(), width);
    }

    if width == 0 {
        return (row, 0);
    }

    let visible_to_cursor = prompt_width + col_chars;
    row += visible_to_cursor.saturating_sub(1) / width;
    let col = (visible_to_cursor.saturating_sub(1) % width) + 1;
    (row, col)
}

/// Render delegation cards stacked vertically between chat and input.
/// Returns the number of lines rendered.
fn render_delegation_bar(stdout: &mut Stdout, state: &ReplState, width: usize) -> u16 {
    if !state.delegation_bar.has_content() {
        return 0;
    }

    let bar = &state.delegation_bar;
    let max_plain_width = width.saturating_sub(8).max(24);
    let mut rendered_lines: u16 = 0;

    for (i, entry) in bar.visible_delegations.iter().enumerate() {
        let is_selected = bar.focused && i == bar.selected;
        let bg = if is_selected {
            BG_HIGHLIGHT
        } else {
            BG_DELEGATION_CARD
        };
        let tree_prefix = if entry.is_parent {
            "↑".to_string()
        } else {
            delegation_tree_prefix(entry.depth)
        };
        let state_icon = if entry.is_parent {
            "←"
        } else if entry.is_busy {
            "⟡"
        } else {
            "○"
        };
        let nav = if is_selected { "▸" } else { " " };
        let mut plain = format!("{nav} {tree_prefix} {state_icon} {}", entry.label);
        if let Some(summary) = delegation_summary(entry) {
            plain.push_str(" · ");
            plain.push_str(&summary);
        }
        plain = truncate_chars(&plain, max_plain_width);

        let styled = if is_selected {
            format!("{WHITE_BOLD}{plain}{RESET}")
        } else if entry.is_busy {
            format!("{ACCENT}{plain}{RESET}")
        } else {
            format!("{DIM}{plain}{RESET}")
        };
        let bg_safe = styled.replace(RESET, &format!("{RESET}{bg}"));
        write!(stdout, "\r\n  {bg} {bg_safe} {RESET}").ok();
        rendered_lines = rendered_lines.saturating_add(1);
    }

    rendered_lines
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
        let phase = ((offset * wave_phase_speed * speed_multiplier) + (i as f32 * wave_wavelength))
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
    nudge_skill_panel: &NudgeSkillPanel,
) {
    let width = term_width() as usize;

    clear_input_area(stdout, completion);

    // ── Top edge: lower half-blocks in input bg color ──
    let fg_input_bg = "\x1b[38;5;235m";
    write!(
        stdout,
        "{fg_input_bg}{}{RESET}",
        HALF_BLOCK_LOWER.to_string().repeat(width)
    )
    .ok();

    // ── Delegation cards (0+ lines between top edge and input) ──
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
            let prompt = format!(
                "  New backend: {ACCENT}{name}{RESET}{BG_INPUT}{DIM} ({short_pk}…){RESET}{BG_INPUT}  {WHITE_BOLD}[a]{RESET}{BG_INPUT}pprove  {WHITE_BOLD}[b]{RESET}{BG_INPUT}lock"
            );
            let visible_len = 2
                + "New backend: ".len()
                + name.len()
                + 2
                + short_pk.len()
                + 2
                + 2
                + "[a]pprove  [b]lock".len();
            let prompt_pad = width.saturating_sub(visible_len);
            write!(
                stdout,
                "\r\n{BG_INPUT}{prompt}{}{RESET}",
                " ".repeat(prompt_pad)
            )
            .ok();
            completion.input_wrap_lines = 0;
            completion.attachment_indicator_lines = 0;
            completion.rendered_lines = 0;

            write!(
                stdout,
                "\r\n{fg_input_bg}{}{RESET}",
                HALF_BLOCK_UPPER.to_string().repeat(width)
            )
            .ok();
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

    // ── Bunker approval prompt (replaces normal input when pending) ──
    if let Some(req) = state.pending_bunker_requests.front() {
        let short_pk = if req.requester_pubkey.len() >= 12 {
            req.requester_pubkey[..12].to_string()
        } else {
            req.requester_pubkey.clone()
        };
        let kind_str = req
            .event_kind
            .map(|k| format!("kind:{k}"))
            .unwrap_or_else(|| "unknown".to_string());
        let content_preview = req
            .event_content
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(40)
            .collect::<String>();

        let line1 = format!(
            "  {ACCENT}Bunker sign request{RESET}{BG_INPUT} from {DIM}{short_pk}…{RESET}{BG_INPUT}  {kind_str}"
        );
        let line1_plain = 2
            + "Bunker sign request".len()
            + " from ".len()
            + short_pk.len()
            + 1
            + 2
            + kind_str.len();
        let line1_pad = width.saturating_sub(line1_plain);
        write!(
            stdout,
            "\r\n{BG_INPUT}{line1}{}{RESET}",
            " ".repeat(line1_pad)
        )
        .ok();

        if !content_preview.is_empty() {
            let line2 = format!("  {DIM}\"{content_preview}\"{RESET}{BG_INPUT}");
            let line2_plain = 2 + 1 + content_preview.chars().count() + 1;
            let line2_pad = width.saturating_sub(line2_plain);
            write!(
                stdout,
                "\r\n{BG_INPUT}{line2}{}{RESET}",
                " ".repeat(line2_pad)
            )
            .ok();
        }

        let prompt = format!(
            "  {WHITE_BOLD}[a]{RESET}{BG_INPUT}pprove  {WHITE_BOLD}[A]{RESET}{BG_INPUT}lways  {WHITE_BOLD}[r]{RESET}{BG_INPUT}eject"
        );
        let prompt_plain = 2 + "[a]pprove  [A]lways  [r]eject".len();
        let prompt_pad = width.saturating_sub(prompt_plain);
        write!(
            stdout,
            "\r\n{BG_INPUT}{prompt}{}{RESET}",
            " ".repeat(prompt_pad)
        )
        .ok();

        completion.input_wrap_lines = 0;
        completion.attachment_indicator_lines = 0;
        completion.rendered_lines = 0;

        write!(
            stdout,
            "\r\n{fg_input_bg}{}{RESET}",
            HALF_BLOCK_UPPER.to_string().repeat(width)
        )
        .ok();
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

        let lines_for_content = if content_preview.is_empty() { 2 } else { 3 };
        queue!(stdout, cursor::MoveUp(lines_for_content + 1)).ok();
        queue!(stdout, cursor::MoveToColumn(prompt_plain as u16)).ok();
        stdout.flush().ok();
        completion.cursor_row = 0;
        completion.input_area_drawn = true;
        return;
    }

    // ── Ask modal (replaces normal input when an ask question is pending) ──
    if let Some(ref ask_modal) = state.ask_modal {
        let input_state = &ask_modal.input_state;
        let mut content_lines: Vec<String> = Vec::new();

        // Tab bar (if multiple questions)
        if input_state.questions.len() > 1 {
            let mut tabs = String::new();
            for (qi, _) in input_state.questions.iter().enumerate() {
                let label = format!("Q{}", qi + 1);
                let answered = input_state.answers.iter().any(|a| a.question_index == qi);
                if qi > 0 {
                    tabs.push_str(&format!("{DIM} │ {RESET}{BG_INPUT}"));
                }
                if qi == input_state.current_question_index {
                    tabs.push_str(&format!("{ACCENT}{WHITE_BOLD} {label} {RESET}{BG_INPUT}"));
                } else if answered {
                    tabs.push_str(&format!("{GREEN} {label} {RESET}{BG_INPUT}"));
                } else {
                    tabs.push_str(&format!("{DIM} {label} {RESET}{BG_INPUT}"));
                }
            }
            content_lines.push(format!("  {tabs}"));
        }

        // Question text
        if let Some(question) = input_state.current_question() {
            let (q_title, q_text) = match question {
                AskQuestion::SingleSelect {
                    title, question, ..
                } => (title.as_str(), question.as_str()),
                AskQuestion::MultiSelect {
                    title, question, ..
                } => (title.as_str(), question.as_str()),
            };
            let multi_label = if input_state.is_multi_select() {
                " (multi-select)"
            } else {
                ""
            };
            content_lines.push(format!(
                "  {WHITE_BOLD}{q_title}{RESET}{BG_INPUT}{DIM}{multi_label}{RESET}{BG_INPUT}"
            ));
            if !q_text.is_empty() && q_text != q_title {
                content_lines.push(format!("  {DIM}{q_text}{RESET}{BG_INPUT}"));
            }

            // Options
            let options: &[String] = match question {
                AskQuestion::SingleSelect { suggestions, .. } => suggestions,
                AskQuestion::MultiSelect { options, .. } => options,
            };
            for (oi, opt) in options.iter().enumerate() {
                let marker = if oi == input_state.selected_option_index {
                    if input_state.is_multi_select() {
                        let checked = input_state
                            .multi_select_state
                            .get(oi)
                            .copied()
                            .unwrap_or(false);
                        if checked {
                            format!("{WHITE_BOLD}▸ ☑{RESET}{BG_INPUT}")
                        } else {
                            format!("{WHITE_BOLD}▸ ☐{RESET}{BG_INPUT}")
                        }
                    } else {
                        format!("{WHITE_BOLD}▸{RESET}{BG_INPUT}")
                    }
                } else if input_state.is_multi_select() {
                    let checked = input_state
                        .multi_select_state
                        .get(oi)
                        .copied()
                        .unwrap_or(false);
                    if checked {
                        format!("{DIM}  ☑{RESET}{BG_INPUT}")
                    } else {
                        format!("{DIM}  ☐{RESET}{BG_INPUT}")
                    }
                } else {
                    format!("{DIM} {RESET}{BG_INPUT}")
                };

                if oi == input_state.selected_option_index {
                    content_lines.push(format!("  {marker} {WHITE_BOLD}{opt}{RESET}{BG_INPUT}"));
                } else {
                    content_lines.push(format!("  {marker} {opt}"));
                }
            }

            // Custom input option
            let custom_idx = options.len();
            let is_custom_selected = input_state.selected_option_index == custom_idx;
            if input_state.mode == AskInputMode::CustomInput {
                let cursor_char = "▏";
                let text = &input_state.custom_input;
                content_lines.push(format!("  {WHITE_BOLD}▸{RESET}{BG_INPUT} {ACCENT}✎ {text}{cursor_char}{RESET}{BG_INPUT}"));
            } else if is_custom_selected {
                content_lines.push(format!("  {WHITE_BOLD}▸{RESET}{BG_INPUT} {ACCENT}✎ Type custom answer...{RESET}{BG_INPUT}"));
            } else {
                content_lines.push(format!("  {DIM}  ✎ Type custom answer...{RESET}{BG_INPUT}"));
            }
        }

        // Help bar
        let help = if input_state.mode == AskInputMode::CustomInput {
            "  Enter: submit  Esc: cancel"
        } else if input_state.is_multi_select() {
            if input_state.questions.len() > 1 {
                "  ↑↓: navigate  Space: toggle  Enter: confirm  ←→: questions  Esc: close"
            } else {
                "  ↑↓: navigate  Space: toggle  Enter: confirm  Esc: close"
            }
        } else if input_state.questions.len() > 1 {
            "  ↑↓: navigate  Enter: select  ←→: questions  Esc: close"
        } else {
            "  ↑↓: navigate  Enter: select  Esc: close"
        };
        content_lines.push(format!("{DIM}{help}{RESET}{BG_INPUT}"));

        // Render all lines with BG_INPUT background
        for line in &content_lines {
            let plain_len = crate::util::strip_ansi(line).chars().count();
            let line_pad = width.saturating_sub(plain_len);
            write!(
                stdout,
                "\r\n{BG_INPUT}{line}{}{RESET}",
                " ".repeat(line_pad)
            )
            .ok();
        }

        completion.input_wrap_lines = 0;
        completion.attachment_indicator_lines = 0;
        completion.rendered_lines = 0;

        // Bottom edge + status bar
        write!(
            stdout,
            "\r\n{fg_input_bg}{}{RESET}",
            HALF_BLOCK_UPPER.to_string().repeat(width)
        )
        .ok();
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

        let total_content_lines = content_lines.len() as u16;
        queue!(stdout, cursor::MoveUp(total_content_lines + 1)).ok();
        queue!(stdout, cursor::MoveToColumn(0)).ok();
        stdout.flush().ok();
        completion.cursor_row = 0;
        completion.input_area_drawn = true;
        return;
    }

    // ── Input lines (dark background, full width; supports explicit newlines) ──
    let display_buffer = if panel.active {
        &panel.origin_command
    } else {
        &editor.buffer
    };
    let show_work_spinner = state.is_current_conversation_busy(runtime);
    let (prompt_str, prompt_width) = if state.search_mode {
        if state.search_all_projects {
            (format!("{WHITE_BOLD}  \u{27f2} all:{RESET}{BG_INPUT} "), 9)
        } else {
            (format!("{WHITE_BOLD}  \u{27f2} {RESET}{BG_INPUT}"), 4)
        }
    } else if show_work_spinner {
        let spinner_frames = ['|', '/', '-', '\\'];
        let frame = spinner_frames[(state.wave_frame as usize) % spinner_frames.len()];
        (
            format!("{WHITE_BOLD}  {frame} {RESET}{BG_INPUT}"),
            PROMPT_PREFIX_WIDTH as usize,
        )
    } else {
        (
            format!("{WHITE_BOLD}  \u{203a} {RESET}{BG_INPUT}"),
            PROMPT_PREFIX_WIDTH as usize,
        )
    };
    let continuation_prefix = " ".repeat(prompt_width);
    for (idx, line) in display_buffer.split('\n').enumerate() {
        let prefix = if idx == 0 {
            &prompt_str
        } else {
            &continuation_prefix
        };
        let visible_len = prompt_width + line.chars().count();
        let last_row_chars = if width > 0 {
            ((visible_len.saturating_sub(1)) % width) + 1
        } else {
            visible_len
        };
        let pad = width.saturating_sub(last_row_chars);
        write!(
            stdout,
            "\r\n{BG_INPUT}{prefix}{line}{}{RESET}",
            " ".repeat(pad)
        )
        .ok();
    }
    completion.input_wrap_lines =
        (input_total_rows(display_buffer, prompt_width, width) as u16).saturating_sub(1);

    // ── Helper + attachment strip (below input) ──
    let mut aux_input_lines: u16 = 0;
    let multiline_send_mode = !panel.active && !state.search_mode && editor.buffer.contains('\n');
    if multiline_send_mode {
        let helper = "  Multiline mode: press Cmd+Enter to send";
        let helper_pad = width.saturating_sub(helper.chars().count());
        write!(
            stdout,
            "\r\n{BG_INPUT}{DIM}{helper}{RESET}{BG_INPUT}{}{RESET}",
            " ".repeat(helper_pad)
        )
        .ok();
        aux_input_lines = aux_input_lines.saturating_add(1);
    }

    // Attachment strip (below input, if attachments or selected skills exist)
    if editor.has_attachments() || !state.selected_skill_ids.is_empty() {
        write!(stdout, "\r\n{BG_INPUT}  📎 ").ok();
        let mut strip_chars: usize = 5;
        for (i, att) in editor.attachments.iter().enumerate() {
            let label = match &att.kind {
                AttachmentKind::Text { content } => {
                    let lines = content.lines().count();
                    format!(
                        "Text Att. {} ({} line{})",
                        att.id,
                        lines,
                        if lines == 1 { "" } else { "s" }
                    )
                }
                AttachmentKind::Image { .. } => format!("Image #{}", att.id),
            };
            let is_selected = editor.selected_attachment == Some(i);
            if is_selected {
                write!(
                    stdout,
                    " {WHITE_BOLD}{BG_HIGHLIGHT} {label} {RESET}{BG_INPUT}"
                )
                .ok();
            } else {
                write!(stdout, " {DIM} {label} {RESET}{BG_INPUT}").ok();
            }
            strip_chars += 2 + label.len() + 1;
        }
        // Skill chips
        {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            for sid in &state.selected_skill_ids {
                let title = store_ref
                    .content
                    .get_skills()
                    .iter()
                    .find(|s| s.id == *sid)
                    .map(|s| s.title.as_str())
                    .unwrap_or("skill");
                let chip = format!("[${}]", title);
                write!(stdout, " {ACCENT}{chip}{RESET}{BG_INPUT}").ok();
                strip_chars += 1 + chip.chars().count();
            }
        }
        let strip_pad = width.saturating_sub(strip_chars);
        write!(stdout, "{}{RESET}", " ".repeat(strip_pad)).ok();
        aux_input_lines = aux_input_lines.saturating_add(1);
    }
    completion.attachment_indicator_lines = aux_input_lines;

    // ── Config panel or completion menu ──
    if panel.active {
        // Header per mode
        let header = match panel.mode {
            PanelMode::Tools => format!("  Tools for {}", panel.agent_name),
            PanelMode::AgentSelect => {
                if panel.filter.is_empty() {
                    "  Select agent".to_string()
                } else {
                    format!("  Select agent ({})", panel.filter)
                }
            }
            PanelMode::FlagSelect => "  Options".to_string(),
            PanelMode::ModelSelect => {
                if panel.filter.is_empty() {
                    format!("  Model for {}", panel.agent_name)
                } else {
                    format!("  Model for {} ({})", panel.agent_name, panel.filter)
                }
            }
        };
        let header_len = header.chars().count();
        let header_pad = width.saturating_sub(header_len);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(
            stdout,
            "{BG_INPUT}{WHITE_BOLD}{header}{}{RESET}",
            " ".repeat(header_pad)
        )
        .ok();

        // Items list — use filtered_items() for the visible subset
        let filtered = panel.filtered_items();
        let max_visible = 15;
        let visible_end = (panel.scroll_offset + max_visible).min(filtered.len());
        let visible_count = visible_end.saturating_sub(panel.scroll_offset);

        for fi in panel.scroll_offset..visible_end {
            let (_orig_idx, item) = filtered[fi];

            // Per-mode markers
            let (marker, marker_len) = match panel.mode {
                PanelMode::Tools => {
                    if panel.tools_selected.contains(item.as_str()) {
                        ("[x] ", 4)
                    } else {
                        ("[ ] ", 4)
                    }
                }
                PanelMode::AgentSelect => ("", 0),
                PanelMode::FlagSelect => ("", 0), // markers are baked into item text
                PanelMode::ModelSelect => {
                    let is_selected = panel.pending_model.as_deref() == Some(item.as_str());
                    if is_selected {
                        (concat!("(*) "), 4)
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
            if fi == panel.cursor {
                write!(
                    stdout,
                    "{BG_HIGHLIGHT}{WHITE_BOLD}{text}{}{RESET}",
                    " ".repeat(item_pad)
                )
                .ok();
            } else {
                write!(
                    stdout,
                    "{BG_INPUT}{DIM}{text}{}{RESET}",
                    " ".repeat(item_pad)
                )
                .ok();
            }
        }

        // Footer per mode
        let footer = match panel.mode {
            PanelMode::Tools => "  Space: toggle  @: agent  -: options  Enter: save  Esc: cancel",
            PanelMode::AgentSelect => "  Enter: select  Type to filter  Esc: back",
            PanelMode::FlagSelect => "  Enter: select  Esc: back",
            PanelMode::ModelSelect => "  Enter: select  Type to filter  Esc: back",
        };
        let footer_len = footer.chars().count();
        let footer_pad = width.saturating_sub(footer_len);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(
            stdout,
            "{BG_INPUT}{DIM}{footer}{}{RESET}",
            " ".repeat(footer_pad)
        )
        .ok();

        completion.rendered_lines = (1 + visible_count + 1) as u16;
    } else if nudge_skill_panel.active {
        let header = if nudge_skill_panel.filter.is_empty() {
            format!("  {WHITE_BOLD}╸Skills╺{RESET}{BG_INPUT}")
        } else {
            format!(
                "  {WHITE_BOLD}╸Skills╺{RESET}{BG_INPUT}  filter: {}",
                nudge_skill_panel.filter
            )
        };
        let header_plain = 10
            + if nudge_skill_panel.filter.is_empty() {
                0
            } else {
                10 + nudge_skill_panel.filter.len()
            };
        let header_pad = width.saturating_sub(header_plain);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(
            stdout,
            "{BG_INPUT}{header}{}{RESET}",
            " ".repeat(header_pad)
        )
        .ok();

        // Items list
        let filtered = nudge_skill_panel.filtered_items();
        let max_visible = 15;
        let visible_end = (nudge_skill_panel.scroll_offset + max_visible).min(filtered.len());
        let visible_count = visible_end.saturating_sub(nudge_skill_panel.scroll_offset);

        for fi in nudge_skill_panel.scroll_offset..visible_end {
            let (_, item) = &filtered[fi];
            let marker = if nudge_skill_panel.selected_ids.contains(&item.id) {
                "[x] "
            } else {
                "[ ] "
            };
            let desc_preview = if item.description.is_empty() {
                String::new()
            } else {
                let short: String = item.description.chars().take(30).collect();
                format!(" {DIM}— {short}{RESET}")
            };
            let text = format!("  {marker}{}{desc_preview}", item.title);
            let text_plain = 2
                + 4
                + item.title.chars().count()
                + if item.description.is_empty() {
                    0
                } else {
                    3 + item.description.chars().take(30).count()
                };
            let item_pad = width.saturating_sub(text_plain);
            write!(stdout, "\r\n").ok();
            queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
            if fi == nudge_skill_panel.cursor {
                write!(
                    stdout,
                    "{BG_HIGHLIGHT}{WHITE_BOLD}{text}{}{RESET}",
                    " ".repeat(item_pad)
                )
                .ok();
            } else {
                write!(
                    stdout,
                    "{BG_INPUT}{DIM}{text}{}{RESET}",
                    " ".repeat(item_pad)
                )
                .ok();
            }
        }

        let footer = "  Space: toggle  Enter: confirm  Esc: cancel";
        let footer_len = footer.chars().count();
        let footer_pad = width.saturating_sub(footer_len);
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
        write!(
            stdout,
            "{BG_INPUT}{DIM}{footer}{}{RESET}",
            " ".repeat(footer_pad)
        )
        .ok();

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
        write!(
            stdout,
            "{BG_INPUT}{tab_bar}{}{RESET}",
            " ".repeat(tab_bar_pad)
        )
        .ok();

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
        write!(
            stdout,
            "{BG_INPUT}{DIM}{footer}{}{RESET}",
            " ".repeat(footer_pad)
        )
        .ok();

        completion.rendered_lines = (2 + visible_count + 1) as u16;
    } else if completion.visible && !completion.items.is_empty() {
        let count = completion.items.len().min(12) as u16;
        for (i, item) in completion.items.iter().take(12).enumerate() {
            let label = &item.label;
            let desc = &item.description;
            write!(stdout, "\r\n").ok();
            queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
            if item.completed {
                let text = format!("  \u{2714} {label:<22} {desc}");
                let text_len = 2 + 2 + label.chars().count().max(22) + 1 + desc.chars().count();
                let item_pad = width.saturating_sub(text_len);
                if i == completion.selected {
                    write!(
                        stdout,
                        "{BG_INPUT}{GREEN}{text}{}{RESET}",
                        " ".repeat(item_pad)
                    )
                    .ok();
                } else {
                    write!(
                        stdout,
                        "{BG_INPUT}\x1b[38;5;242m{text}{}{RESET}",
                        " ".repeat(item_pad)
                    )
                    .ok();
                }
            } else {
                let text = format!("  {label:<24} {desc}");
                let text_len = 2 + label.chars().count().max(24) + 1 + desc.chars().count();
                let item_pad = width.saturating_sub(text_len);
                if i == completion.selected {
                    write!(
                        stdout,
                        "{BG_INPUT}{WHITE_BOLD}{text}{}{RESET}",
                        " ".repeat(item_pad)
                    )
                    .ok();
                } else {
                    write!(
                        stdout,
                        "{BG_INPUT}{DIM}{text}{}{RESET}",
                        " ".repeat(item_pad)
                    )
                    .ok();
                }
            }
        }
        completion.rendered_lines = count;
    } else {
        completion.rendered_lines = 0;
    }

    // ── Bottom edge: upper half-blocks in input bg color ──
    let fg_input_bg = "\x1b[38;5;235m";
    write!(
        stdout,
        "\r\n{fg_input_bg}{}{RESET}",
        HALF_BLOCK_UPPER.to_string().repeat(width)
    )
    .ok();

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

        let (nav_text, nav_plain_width) =
            state.status_bar_text_navigable(runtime, status_nav.segment);
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
    let (cursor_row, col) = if panel.active {
        input_cursor_position(display_buffer, 0, prompt_width, width)
    } else {
        input_cursor_position(display_buffer, editor.cursor, prompt_width, width)
    };
    let wrap_lines_after_cursor = (completion.input_wrap_lines as usize).saturating_sub(cursor_row);
    queue!(
        stdout,
        cursor::MoveUp(lines_below + wrap_lines_after_cursor as u16)
    )
    .ok();
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
    write!(stdout, "{RESET}").ok();
    queue!(stdout, terminal::Clear(ClearType::FromCursorDown)).ok();
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
    nudge_skill_panel: &NudgeSkillPanel,
) {
    state.search_mode = false;
    state.search_all_projects = false;
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )
    .ok();
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
    redraw_input(
        stdout,
        state,
        runtime,
        editor,
        completion,
        panel,
        status_nav,
        stats_panel,
        nudge_skill_panel,
    );
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
    nudge_skill_panel: &NudgeSkillPanel,
) {
    clear_input_area(stdout, completion);
    completion.input_area_drawn = false;

    for line in text.split('\n') {
        write!(stdout, "{}\r\n", line).ok();
    }
    stdout.flush().ok();

    redraw_input(
        stdout,
        state,
        runtime,
        editor,
        completion,
        panel,
        status_nav,
        stats_panel,
        nudge_skill_panel,
    );
}
