use std::collections::HashSet;
use std::io::{Stdout, Write};
use crate::{DIM, GREEN, WHITE_BOLD, CYAN, RED, RESET};
use crate::editor::LineEditor;
use crate::completion::CompletionMenu;
use crate::panels::{ConfigPanel, ConversationStackEntry, StatusBarNav, StatsPanel, NudgeSkillPanel};
use crate::state::{AskModalState, ReplState};
use crate::format::{format_message, print_separator_raw, print_user_message_raw, print_error_raw, print_system_raw, is_tool_use};
use crate::render::{print_above_input, redraw_input};
use crate::util::{thread_display_name, MESSAGES_TO_LOAD};
use tenex_core::runtime::CoreRuntime;
use tenex_core::nostr::NostrCommand;
use tenex_core::models::{AskInputState, Project, ProjectAgent, Thread};
use tenex_core::events::CoreEvent;
use nostr_sdk::prelude::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Find the longest project title that matches as a prefix of `input` (case-insensitive).
/// Returns `(matched_portion, remainder)` where remainder is trimmed.
pub(crate) fn find_project_split<'a>(input: &'a str, projects: &[&Project]) -> Option<(&'a str, &'a str)> {
    let mut best_len = 0;
    for project in projects {
        let title = &project.title;
        let title_len = title.len();
        if title_len > best_len
            && title_len <= input.len()
            && input[..title_len].eq_ignore_ascii_case(title)
            && (title_len == input.len() || input.as_bytes()[title_len] == b' ')
        {
            best_len = title_len;
        }
    }

    if best_len > 0 {
        Some((&input[..best_len], input[best_len..].trim_start()))
    } else {
        None
    }
}

// ─── Command Result ─────────────────────────────────────────────────────────

pub(crate) enum CommandResult {
    Lines(Vec<String>),
    /// Set the buffer to this string and show completion menu
    ShowCompletion(String),
    /// Clear the entire screen, optionally print lines, reposition input at bottom
    ClearScreen(Vec<String>),
}

// ─── Command Handlers ───────────────────────────────────────────────────────

pub(crate) fn handle_project_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let mut output = Vec::new();
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let projects: Vec<&Project> = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .collect();

    match arg {
        None | Some("") => {
            if projects.is_empty() {
                output.push(print_system_raw("No projects found. Waiting for sync..."));
                return output;
            }
            output.push(format!("{WHITE_BOLD}Projects:{RESET}"));
            for (i, project) in projects.iter().enumerate() {
                let marker = if state.current_project.as_deref() == Some(&project.a_tag()) {
                    format!("{GREEN}*{RESET} ")
                } else {
                    "  ".to_string()
                };
                let online = if store_ref.is_project_online(&project.a_tag()) {
                    format!(" {GREEN}(online){RESET}")
                } else {
                    format!(" {DIM}(offline){RESET}")
                };
                output.push(format!("  {marker}{}: {}{online}", i + 1, project.title));
            }
        }
        Some(name) => {
            let matched = if let Ok(idx) = name.parse::<usize>() {
                projects.get(idx.saturating_sub(1)).copied()
            } else {
                let lower = name.to_lowercase();
                projects
                    .iter()
                    .find(|p| p.title.to_lowercase() == lower)
                    .or_else(|| projects.iter().find(|p| p.title.to_lowercase().contains(&lower)))
                    .copied()
            };

            match matched {
                Some(project) => {
                    let a_tag = project.a_tag();
                    let title = project.title.clone();
                    drop(store_ref);

                    switch_to_project(state, runtime, &a_tag);
                    state.current_conversation = None;
                    state.last_displayed_pubkey = None;
                    state.last_todo_items.clear();
                    output.push(print_system_raw(&format!("Switched to project: {title}")));
                }
                None => output.push(print_error_raw(&format!("No project matching '{name}'"))),
            }
        }
    }
    output
}

/// Open a conversation: set it as current, load recent messages, return display lines.
pub(crate) fn open_conversation(
    state: &mut ReplState,
    runtime: &CoreRuntime,
    thread_id: &str,
    clear_stack: bool,
    max_messages: usize,
) -> Vec<String> {
    state.current_conversation = Some(thread_id.to_string());
    if clear_stack {
        state.conversation_stack.clear();
    }
    state.delegation_bar.unfocus();
    state.last_todo_items.clear();

    let mut output = Vec::new();

    let store = runtime.data_store();
    let store_ref = store.borrow();

    let messages = store_ref.get_messages(thread_id);
    let show_count = if max_messages == 0 { messages.len() } else { messages.len().min(max_messages) };
    let start = messages.len().saturating_sub(show_count);

    if show_count > 0 {
        if start > 0 {
            output.push(print_system_raw(&format!("  ... {} earlier messages", start)));
        }
        let mut last_pk: Option<String> = None;
        let mut todo_items: Vec<(String, String)> = Vec::new();
        for msg in &messages[start..] {
            if let Some(formatted) = format_message(msg, &store_ref, &state.user_pubkey, &mut last_pk, &mut todo_items) {
                output.push(formatted);
            }
        }
        state.last_displayed_pubkey = last_pk;
        state.last_todo_items = todo_items;
        output.push(print_separator_raw());
    }

    output
}

/// Check for unanswered ask events in the current thread and open the ask modal if found.
pub(crate) fn maybe_open_ask_modal(state: &mut ReplState, runtime: &CoreRuntime) {
    if state.ask_modal.is_some() {
        return;
    }
    let thread_id = match &state.current_conversation {
        Some(id) => id.clone(),
        None => return,
    };

    let store = runtime.data_store();
    let store_ref = store.borrow();
    if let Some((ask_event_id, ask_event, author_pubkey)) =
        store_ref.get_unanswered_ask_for_thread(&thread_id)
    {
        state.ask_modal = Some(AskModalState {
            message_id: ask_event_id,
            input_state: AskInputState::new(ask_event.questions),
            ask_author_pubkey: author_pubkey,
        });
    }
}

pub(crate) fn subscribe_to_project(runtime: &CoreRuntime, a_tag: &str) {
    let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMessages {
        project_a_tag: a_tag.to_string(),
    });
    let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMetadata {
        project_a_tag: a_tag.to_string(),
    });
}

/// Unified project switching: subscribe + switch state + auto-select agent.
pub(crate) fn switch_to_project(state: &mut ReplState, runtime: &CoreRuntime, a_tag: &str) {
    subscribe_to_project(runtime, a_tag);
    state.switch_project(a_tag.to_string(), runtime);
    auto_select_agent(state, runtime);
}

/// Auto-select the first online project. Returns true if a project was selected.
pub(crate) fn auto_select_project(state: &mut ReplState, runtime: &CoreRuntime) -> bool {
    if state.current_project.is_some() {
        return false;
    }
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let online_project = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .find(|p| store_ref.is_project_online(&p.a_tag()))
        .map(|p| (p.a_tag(), p.title.clone()));
    drop(store_ref);

    if let Some((a_tag, title)) = online_project {
        switch_to_project(state, runtime, &a_tag);
        raw_println!("{}", print_system_raw(&format!("Auto-selected project: {title}")));
        true
    } else {
        false
    }
}

pub(crate) fn auto_select_agent(state: &mut ReplState, runtime: &CoreRuntime) {
    let Some(ref a_tag) = state.current_project else {
        return;
    };
    let store = runtime.data_store();
    let store_ref = store.borrow();
    if let Some(agents) = store_ref.get_online_agents(a_tag) {
        let agent = agents
            .iter()
            .find(|a| a.is_pm)
            .or_else(|| agents.first());
        if let Some(a) = agent {
            state.current_agent = Some(a.pubkey.clone());
            state.current_agent_name = Some(a.name.clone());
        }
    }
}

pub(crate) fn handle_agent_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let mut output = Vec::new();

    let agent_arg = if let Some(raw) = arg {
        if let Some(at_pos) = raw.find('@') {
            let after_at = raw[at_pos + 1..].trim();

            let store = runtime.data_store();
            let store_ref = store.borrow();
            let projects: Vec<&Project> = store_ref
                .get_projects()
                .iter()
                .filter(|p| !p.is_deleted)
                .collect();

            let (project_name, remainder) = match find_project_split(after_at, &projects) {
                Some((proj, rest)) => (proj, if rest.is_empty() { None } else { Some(rest) }),
                None => match after_at.find(' ') {
                    Some(sp) => (&after_at[..sp], Some(after_at[sp + 1..].trim())),
                    None => (after_at, None),
                },
            };

            let lower = project_name.to_lowercase();
            let matched = projects
                .iter()
                .find(|p| p.title.to_lowercase() == lower)
                .or_else(|| projects.iter().find(|p| p.title.to_lowercase().contains(&lower)))
                .map(|p| p.a_tag());
            drop(store_ref);

            match matched {
                Some(a_tag) => {
                    switch_to_project(state, runtime, &a_tag);
                }
                None => {
                    output.push(print_error_raw(&format!("No project matching '{project_name}'")));
                    return output;
                }
            }

            remainder
        } else {
            arg
        }
    } else {
        arg
    };

    let Some(ref a_tag) = state.current_project else {
        output.push(print_error_raw("Select a project first with /project"));
        return output;
    };
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let agents: Vec<&ProjectAgent> = store_ref
        .get_online_agents(a_tag)
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    match agent_arg {
        None | Some("") => {
            if agents.is_empty() {
                output.push(print_system_raw("No online agents. Is the backend running?"));
                return output;
            }
            output.push(format!("{WHITE_BOLD}Online agents:{RESET}"));
            for (i, agent) in agents.iter().enumerate() {
                let marker = if state.current_agent.as_deref() == Some(&agent.pubkey) {
                    format!("{GREEN}*{RESET} ")
                } else {
                    "  ".to_string()
                };
                let model = agent.model.as_deref().unwrap_or("unknown model");
                let pm_badge = if agent.is_pm { " [PM]" } else { "" };
                output.push(format!(
                    "  {marker}{}: {}{pm_badge} ({DIM}{model}{RESET})",
                    i + 1,
                    agent.name
                ));
            }
        }
        Some(name) => {
            let matched = if let Ok(idx) = name.parse::<usize>() {
                agents.get(idx.saturating_sub(1)).copied()
            } else {
                let lower = name.to_lowercase();
                agents
                    .iter()
                    .find(|a| a.name.to_lowercase().contains(&lower))
                    .copied()
            };

            match matched {
                Some(agent) => {
                    state.current_agent = Some(agent.pubkey.clone());
                    state.current_agent_name = Some(agent.name.clone());
                    output.push(print_system_raw(&format!("Switched to agent: {}", agent.name)));
                }
                None => output.push(print_error_raw(&format!("No agent matching '{name}'"))),
            }
        }
    }
    output
}

pub(crate) fn handle_open_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> CommandResult {
    let arg = arg.unwrap_or("");
    if arg.is_empty() {
        return CommandResult::ShowCompletion("/conversations ".to_string());
    }

    let (project_switch, idx_str) = if let Some(at_pos) = arg.find('@') {
        let after_at = arg[at_pos + 1..].trim();
        match after_at.find(' ') {
            Some(sp) => (Some(after_at[..sp].trim()), after_at[sp + 1..].trim()),
            None => return CommandResult::ShowCompletion(format!("/conversations {arg}")),
        }
    } else {
        (None, arg.trim())
    };

    if let Some(project_name) = project_switch {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let lower = project_name.to_lowercase();
        let matched = store_ref
            .get_projects()
            .iter()
            .filter(|p| !p.is_deleted)
            .find(|p| p.title.to_lowercase().contains(&lower))
            .map(|p| p.a_tag());
        drop(store_ref);

        match matched {
            Some(a_tag) => {
                switch_to_project(state, runtime, &a_tag);
            }
            None => return CommandResult::Lines(vec![print_error_raw(&format!("No project matching '{project_name}'"))]),
        }
    }

    let Some(ref a_tag) = state.current_project else {
        return CommandResult::Lines(vec![print_error_raw("Select a project first with /project")]);
    };

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let mut threads: Vec<&Thread> = store_ref.get_threads(a_tag).iter().collect();
    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));

    let matched = if let Ok(idx) = idx_str.parse::<usize>() {
        threads.get(idx.saturating_sub(1)).copied()
    } else {
        let lower = idx_str.strip_suffix("...").unwrap_or(idx_str).to_lowercase();
        threads.iter().find(|t|
            t.title.to_lowercase().contains(&lower)
            || t.summary.as_ref().map(|s| s.to_lowercase().contains(&lower)).unwrap_or(false)
        ).copied()
    };

    let Some(thread) = matched else {
        return CommandResult::Lines(vec![print_error_raw(&format!("No conversation matching '{idx_str}'"))]);
    };

    let id = thread.id.clone();
    let title = thread.title.clone();
    drop(store_ref);

    let mut output = vec![print_system_raw(&format!("Opened: {title}"))];
    output.extend(open_conversation(state, runtime, &id, true, MESSAGES_TO_LOAD));
    CommandResult::ClearScreen(output)
}

pub(crate) fn handle_active_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> CommandResult {
    let arg = arg.unwrap_or("");
    if arg.is_empty() {
        return CommandResult::ShowCompletion("/active ".to_string());
    }

    let search = arg.trim();

    let store = runtime.data_store();
    let store_ref = store.borrow();

    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut ordered_threads: Vec<(String, Option<String>)> = Vec::new();

    let active_ops = store_ref.operations.get_all_active_operations();
    for op in &active_ops {
        let thread_id = op.thread_id.as_deref().unwrap_or(&op.event_id);
        if !seen_ids.insert(thread_id.to_string()) {
            continue;
        }
        if store_ref.get_thread_by_id(thread_id).is_none() {
            seen_ids.remove(thread_id);
            continue;
        }
        let project_a_tag = store_ref.get_project_a_tag_for_thread(thread_id);
        ordered_threads.push((thread_id.to_string(), project_a_tag));
    }

    let mut recent_threads: Vec<(&Thread, String)> = Vec::new();
    for project in store_ref.get_projects().iter().filter(|p| !p.is_deleted) {
        let a_tag = project.a_tag();
        for thread in store_ref.get_threads(&a_tag) {
            if !seen_ids.contains(&thread.id) {
                recent_threads.push((thread, a_tag.clone()));
            }
        }
    }
    recent_threads.sort_by(|a, b| b.0.effective_last_activity.cmp(&a.0.effective_last_activity));

    for (thread, a_tag) in recent_threads.into_iter().take(15) {
        if !seen_ids.insert(thread.id.clone()) {
            continue;
        }
        ordered_threads.push((thread.id.clone(), Some(a_tag)));
    }

    let matched_entry = if let Ok(idx) = search.parse::<usize>() {
        ordered_threads.get(idx.saturating_sub(1))
    } else {
        let lower = search.to_lowercase();
        ordered_threads.iter().find(|(tid, _)| {
            store_ref.get_thread_by_id(tid).map(|t|
                t.title.to_lowercase().contains(&lower)
                || t.summary.as_ref().map(|s| s.to_lowercase().contains(&lower)).unwrap_or(false)
            ).unwrap_or(false)
        })
    };

    let Some((thread_id, project_a_tag)) = matched_entry else {
        return CommandResult::Lines(vec![print_error_raw(&format!("No conversation matching '{search}'"))]);
    };

    let thread_id = thread_id.clone();
    let Some(thread) = store_ref.get_thread_by_id(&thread_id) else {
        return CommandResult::Lines(vec![print_error_raw("Conversation not found")]);
    };
    let title = thread.title.clone();

    let needs_project_switch = project_a_tag
        .as_ref()
        .map(|a| state.current_project.as_ref() != Some(a))
        .unwrap_or(false);
    let switch_a_tag = project_a_tag.clone();

    drop(store_ref);

    if needs_project_switch {
        if let Some(a_tag) = &switch_a_tag {
            switch_to_project(state, runtime, a_tag);
        }
    }

    let mut output = vec![print_system_raw(&format!("Opened: {title}"))];
    output.extend(open_conversation(state, runtime, &thread_id, true, MESSAGES_TO_LOAD));

    if needs_project_switch {
        CommandResult::ClearScreen(output)
    } else {
        CommandResult::Lines(output)
    }
}

pub(crate) fn handle_new_command(arg: &str, state: &mut ReplState, runtime: &CoreRuntime) -> CommandResult {
    let arg = arg.trim();

    if let Some(at_pos) = arg.find('@') {
        let agent_part = arg[..at_pos].trim();
        let after_at = arg[at_pos + 1..].trim();

        // When agent is already specified before @, full after_at is the project name
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let projects: Vec<&Project> = store_ref
            .get_projects()
            .iter()
            .filter(|p| !p.is_deleted)
            .collect();

        let (project_part, agent_override) = if !agent_part.is_empty() {
            (after_at, None)
        } else {
            match find_project_split(after_at, &projects) {
                Some((proj, rest)) => (proj, if rest.is_empty() { None } else { Some(rest) }),
                None => match after_at.find(' ') {
                    Some(sp) => (&after_at[..sp], Some(after_at[sp + 1..].trim())),
                    None => (after_at, None),
                },
            }
        };

        let agent_name = if !agent_part.is_empty() {
            agent_part
        } else {
            agent_override.unwrap_or("")
        };

        let project_a_tag = if !project_part.is_empty() {
            let lower = project_part.to_lowercase();
            projects
                .iter()
                .find(|p| p.title.to_lowercase() == lower)
                .or_else(|| projects.iter().find(|p| p.title.to_lowercase().contains(&lower)))
                .map(|p| p.a_tag())
        } else {
            None
        };
        drop(store_ref);

        if let Some(a_tag) = project_a_tag {
            switch_to_project(state, runtime, &a_tag);
        }

        if !agent_name.is_empty() {
            if let Some(ref a_tag) = state.current_project {
                let store = runtime.data_store();
                let store_ref = store.borrow();
                if let Some(agents) = store_ref.get_online_agents(a_tag) {
                    let lower = agent_name.to_lowercase();
                    if let Some(agent) = agents.iter().find(|a| a.name.to_lowercase().contains(&lower)) {
                        state.current_agent = Some(agent.pubkey.clone());
                        state.current_agent_name = Some(agent.name.clone());
                    }
                }
            }
        }
    } else if !arg.is_empty() {
        if let Some(ref a_tag) = state.current_project {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            if let Some(agents) = store_ref.get_online_agents(a_tag) {
                let lower = arg.to_lowercase();
                if let Some(agent) = agents.iter().find(|a| a.name.to_lowercase().contains(&lower)) {
                    state.current_agent = Some(agent.pubkey.clone());
                    state.current_agent_name = Some(agent.name.clone());
                }
            }
        }
    }

    state.current_conversation = None;
    state.last_displayed_pubkey = None;
    state.conversation_stack.clear();
    state.delegation_bar.unfocus();
    state.last_todo_items.clear();
    CommandResult::ClearScreen(vec![])
}

/// Navigate into a delegation: push current conversation onto the stack,
/// switch to the delegation's conversation, and return lines to display.
pub(crate) fn navigate_to_delegation(state: &mut ReplState, runtime: &CoreRuntime, target_thread_id: &str) -> CommandResult {
    if let Some(ref conv_id) = state.current_conversation {
        state.conversation_stack.push(ConversationStackEntry {
            thread_id: conv_id.clone(),
            project_a_tag: state.current_project.clone(),
        });
    }

    let mut output = Vec::new();
    {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        if let Some(thread) = store_ref.get_thread_by_id(target_thread_id) {
            let title = thread_display_name(thread, 50);
            output.push(print_system_raw(&format!("→ Delegation: {title}")));
        }
    }

    output.extend(open_conversation(state, runtime, target_thread_id, false, MESSAGES_TO_LOAD));
    CommandResult::ClearScreen(output)
}

/// Handle opening a conversation from the status bar navigation.
pub(crate) fn handle_status_bar_open(
    state: &mut ReplState,
    runtime: &CoreRuntime,
    thread_id: &str,
    project_a_tag: Option<String>,
) -> CommandResult {
    if let Some(a_tag) = &project_a_tag {
        if state.current_project.as_ref() != Some(a_tag) {
            switch_to_project(state, runtime, a_tag);
        }
    }
    let title = {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        store_ref.get_thread_by_id(thread_id)
            .map(|t| t.title.clone())
            .unwrap_or_default()
    };
    let mut output = vec![print_system_raw(&format!("Opened: {title}"))];
    output.extend(open_conversation(state, runtime, thread_id, true, MESSAGES_TO_LOAD));
    CommandResult::ClearScreen(output)
}

/// Pop the conversation stack and return to the previous conversation.
pub(crate) fn pop_conversation_stack(state: &mut ReplState, runtime: &CoreRuntime) -> Option<CommandResult> {
    let entry = state.conversation_stack.pop()?;

    if let Some(ref a_tag) = entry.project_a_tag {
        if state.current_project.as_ref() != Some(a_tag) {
            switch_to_project(state, runtime, a_tag);
        }
    }

    let mut output = vec![print_system_raw("← Back to parent conversation")];
    output.extend(open_conversation(state, runtime, &entry.thread_id, false, MESSAGES_TO_LOAD));
    Some(CommandResult::ClearScreen(output))
}

pub(crate) fn handle_send_message(content: &str, state: &mut ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let Some(ref a_tag) = state.current_project else {
        return vec![print_error_raw("Select a project first with /project")];
    };

    if state.current_conversation.is_none() {
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);

        let _ = runtime.handle().send(NostrCommand::PublishThread {
            project_a_tag: a_tag.clone(),
            title: String::new(),
            content: content.to_string(),
            agent_pubkey: state.current_agent.clone(),
            nudge_ids: state.selected_nudge_ids.clone(),
            skill_ids: state.selected_skill_ids.clone(),
            reference_conversation_id: None,
            reference_report_a_tag: None,
            fork_message_id: None,
            response_tx: Some(response_tx),
        });

        if let Ok(event_id) = response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            state.current_conversation = Some(event_id);
            state.last_todo_items.clear();
        }

        state.selected_nudge_ids.clear();
        state.selected_skill_ids.clear();
        state.last_displayed_pubkey = Some(state.user_pubkey.clone());
        return vec![print_user_message_raw(content)];
    }

    let thread_id = state.current_conversation.as_ref().unwrap();

    let _ = runtime.handle().send(NostrCommand::PublishMessage {
        thread_id: thread_id.clone(),
        project_a_tag: a_tag.clone(),
        content: content.to_string(),
        agent_pubkey: state.current_agent.clone(),
        reply_to: None,
        nudge_ids: state.selected_nudge_ids.clone(),
        skill_ids: state.selected_skill_ids.clone(),
        ask_author_pubkey: None,
        response_tx: None,
    });

    state.selected_nudge_ids.clear();
    state.selected_skill_ids.clear();
    state.last_displayed_pubkey = Some(state.user_pubkey.clone());
    vec![print_user_message_raw(content)]
}

pub(crate) fn handle_boot_command(arg: Option<&str>, runtime: &CoreRuntime) -> Vec<String> {
    let arg = arg.unwrap_or("").trim();
    if arg.is_empty() {
        return vec![print_error_raw("Usage: /boot <project>")];
    }

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let projects: Vec<&Project> = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .collect();

    let matched = if let Ok(idx) = arg.parse::<usize>() {
        projects.get(idx.saturating_sub(1)).copied()
    } else {
        let lower = arg.to_lowercase();
        projects.iter().find(|p| p.title.to_lowercase().contains(&lower)).copied()
    };

    let Some(project) = matched else {
        return vec![print_error_raw(&format!("No project matching '{arg}'"))];
    };

    if store_ref.is_project_online(&project.a_tag()) {
        return vec![print_system_raw(&format!("{} is already online", project.title))];
    }

    let a_tag = project.a_tag();
    let pubkey = project.pubkey.clone();
    let title = project.title.clone();
    drop(store_ref);

    let _ = runtime.handle().send(NostrCommand::BootProject {
        project_a_tag: a_tag,
        project_pubkey: Some(pubkey),
    });

    vec![print_system_raw(&format!("Booting {}...", title))]
}

pub(crate) fn handle_status_command(state: &ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let mut output = Vec::new();
    let project = state.project_display(runtime);
    let agent = state.agent_display();
    let conv = match &state.current_conversation {
        Some(id) => {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            store_ref
                .get_thread_by_id(id)
                .map(|t| t.title.clone())
                .unwrap_or_else(|| format!("{}...", &id[..id.len().min(12)]))
        }
        None => "none".to_string(),
    };

    output.push(format!("{WHITE_BOLD}Status:{RESET}"));
    output.push(format!("  Project:      {CYAN}{project}{RESET}"));
    output.push(format!("  Agent:        {GREEN}{agent}{RESET}"));
    output.push(format!("  Conversation: {conv}"));

    if let Some(ref a_tag) = state.current_project {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let online = store_ref.is_project_online(a_tag);
        let status = if online {
            format!("{GREEN}online{RESET}")
        } else {
            format!("{RED}offline{RESET}")
        };
        output.push(format!("  Backend:      {status}"));
    }
    output
}

// ─── Config / Model Command Handlers ────────────────────────────────────────

fn resolve_agent_for_config(
    filter: &str,
    state: &ReplState,
    runtime: &CoreRuntime,
    a_tag: &str,
) -> Result<(String, String), String> {
    if filter.is_empty() {
        match (&state.current_agent, &state.current_agent_name) {
            (Some(pk), Some(name)) => Ok((pk.clone(), name.clone())),
            _ => Err("No current agent. Specify an agent or switch with /agent".to_string()),
        }
    } else {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let agents: Vec<&ProjectAgent> = store_ref
            .get_online_agents(a_tag)
            .map(|a| a.iter().collect())
            .unwrap_or_default();

        let matched = if let Ok(idx) = filter.parse::<usize>() {
            if idx == 0 { None } else { agents.get(idx - 1).copied() }
        } else {
            let lower = filter.to_lowercase();
            agents.iter().find(|a| a.name.to_lowercase().contains(&lower)).copied()
        };

        match matched {
            Some(agent) => Ok((agent.pubkey.clone(), agent.name.clone())),
            None => Err(format!("No agent matching '{filter}'")),
        }
    }
}

pub(crate) fn handle_config_command(
    arg: Option<&str>,
    state: &mut ReplState,
    runtime: &CoreRuntime,
    panel: &mut ConfigPanel,
) -> CommandResult {
    let raw = arg.unwrap_or("");

    let Some(ref a_tag) = state.current_project else {
        return CommandResult::Lines(vec![print_error_raw("Select a project first with /project")]);
    };
    let a_tag = a_tag.clone();

    let mut open_model = false;
    let mut is_set_pm = false;
    let mut is_global = false;
    let mut agent_filter = String::new();
    let mut open_agent_select = false;

    for part in raw.split_whitespace() {
        match part {
            "--model" => open_model = true,
            "--set-pm" => is_set_pm = true,
            "--global" => is_global = true,
            _ if part.starts_with('@') => {
                let name = &part[1..];
                if name.is_empty() {
                    open_agent_select = true;
                } else {
                    agent_filter = name.to_string();
                }
            }
            _ => agent_filter = part.to_string(),
        }
    }

    // If @ alone was specified, we need to open agent select first
    // but we still need a project_a_tag on the panel
    let (agent_pubkey, agent_name) = if open_agent_select {
        // Agent not yet resolved — will be picked in AgentSelect mode
        // Use current agent as placeholder so we have something to fall back to
        match (&state.current_agent, &state.current_agent_name) {
            (Some(pk), Some(name)) => (pk.clone(), name.clone()),
            _ => (String::new(), String::new()),
        }
    } else {
        match resolve_agent_for_config(&agent_filter, state, runtime, &a_tag) {
            Ok(pair) => pair,
            Err(msg) => return CommandResult::Lines(vec![print_error_raw(&msg)]),
        }
    };

    // Load tools for the resolved agent
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let status = store_ref.get_project_status(&a_tag);
    let agent = status.and_then(|s| s.agents.iter().find(|a| a.pubkey == agent_pubkey));

    panel.tools_items = status.map(|s| s.all_tools().iter().map(|t| t.to_string()).collect()).unwrap_or_default();
    panel.tools_selected = agent.map(|a| a.tools.iter().cloned().collect()).unwrap_or_default();
    panel.pending_model = None;
    panel.is_set_pm = is_set_pm || agent.map(|a| a.is_pm).unwrap_or(false);
    panel.filter.clear();
    panel.quick_save = false;

    // If no tools available and not opening agent/model select, bail
    if panel.tools_items.is_empty() && !open_agent_select && !open_model {
        drop(store_ref);
        return CommandResult::Lines(vec![print_error_raw("No tools available for this project")]);
    }

    drop(store_ref);

    panel.active = true;
    panel.agent_pubkey = agent_pubkey;
    panel.agent_name = agent_name;
    panel.project_a_tag = a_tag;
    panel.is_global = is_global;
    panel.cursor = 0;
    panel.scroll_offset = 0;
    panel.origin_command = format!("/config{}", if raw.is_empty() { String::new() } else { format!(" {raw}") });

    // Decide initial mode
    if open_agent_select {
        panel.switch_to_agent_select(runtime);
    } else if open_model {
        panel.switch_to_model_select(runtime);
    } else {
        panel.switch_to_tools();
    }

    CommandResult::Lines(vec![])
}

pub(crate) fn handle_model_command(
    arg: Option<&str>,
    state: &mut ReplState,
    runtime: &CoreRuntime,
    panel: &mut ConfigPanel,
) -> CommandResult {
    let agent_filter = arg.unwrap_or("").trim();

    let Some(ref a_tag) = state.current_project else {
        return CommandResult::Lines(vec![print_error_raw("Select a project first with /project")]);
    };
    let a_tag = a_tag.clone();

    let (agent_pubkey, agent_name) = match resolve_agent_for_config(agent_filter, state, runtime, &a_tag) {
        Ok(pair) => pair,
        Err(msg) => return CommandResult::Lines(vec![print_error_raw(&msg)]),
    };

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let status = store_ref.get_project_status(&a_tag);
    let agent = status.and_then(|s| s.agents.iter().find(|a| a.pubkey == agent_pubkey));

    panel.tools_items = status.map(|s| s.all_tools().iter().map(|t| t.to_string()).collect()).unwrap_or_default();
    panel.tools_selected = agent.map(|a| a.tools.iter().cloned().collect()).unwrap_or_default();
    panel.pending_model = None;
    panel.is_set_pm = agent.map(|a| a.is_pm).unwrap_or(false);
    panel.filter.clear();
    panel.quick_save = true;

    drop(store_ref);

    panel.active = true;
    panel.agent_pubkey = agent_pubkey;
    panel.agent_name = agent_name;
    panel.project_a_tag = a_tag;
    panel.is_global = false;
    panel.cursor = 0;
    panel.scroll_offset = 0;
    panel.origin_command = format!("/model{}", if agent_filter.is_empty() { String::new() } else { format!(" {agent_filter}") });

    panel.switch_to_model_select(runtime);

    if panel.items.is_empty() {
        panel.deactivate();
        return CommandResult::Lines(vec![print_error_raw("No models available for this project")]);
    }

    CommandResult::Lines(vec![])
}

// ─── Event Handlers ─────────────────────────────────────────────────────────

pub(crate) fn handle_core_event(event: &CoreEvent, state: &mut ReplState, runtime: &CoreRuntime, history_store: &crate::history::HistoryStore) -> Option<String> {
    match event {
        CoreEvent::Message(msg) => {
            if msg.pubkey == state.user_pubkey {
                let project_atag = {
                    let store = runtime.data_store();
                    let store_ref = store.borrow();
                    store_ref.get_project_a_tag_for_thread(&msg.thread_id)
                };
                history_store.import_kind1(
                    &msg.content,
                    project_atag.as_deref(),
                    &msg.id,
                    msg.created_at as i64,
                ).ok();

                if state.current_conversation.is_none() {
                    let store = runtime.data_store();
                    let store_ref = store.borrow();
                    if let Some(thread) = store_ref.get_thread_by_id(&msg.thread_id) {
                        if thread.pubkey == state.user_pubkey {
                            drop(store_ref);
                            state.current_conversation = Some(msg.thread_id.clone());
                            state.last_todo_items.clear();
                        }
                    }
                }
                return None;
            }

            if state.current_conversation.as_deref() != Some(&msg.thread_id) {
                return None;
            }

            if msg.is_reasoning {
                return None;
            }

            if state.streaming_in_progress {
                state.streaming_in_progress = false;
                state.stream_buffer.clear();
                state.stream_finished_conv = None;
                state.last_displayed_pubkey = Some(msg.pubkey.clone());
                return Some(format!("\n{}", print_separator_raw()));
            }

            // Suppress duplicate: streaming already displayed this content
            if state.stream_finished_conv.as_deref() == Some(&msg.thread_id)
                && !is_tool_use(msg)
            {
                state.stream_finished_conv = None;
                return None;
            }

            let store = runtime.data_store();
            let store_ref = store.borrow();
            let formatted = format_message(msg, &store_ref, &state.user_pubkey, &mut state.last_displayed_pubkey, &mut state.last_todo_items);
            drop(store_ref);

            formatted.map(|f| {
                if is_tool_use(msg) {
                    f
                } else {
                    format!("{f}\n{}", print_separator_raw())
                }
            })
        }
        _ => None,
    }
}

// ─── Image Upload ───────────────────────────────────────────────────────────

pub(crate) enum UploadResult {
    Success(String),
    Error(String),
}

/// Decode percent-encoded file paths (e.g. from file:// URLs)
fn urlencoded_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let mut hex = String::with_capacity(2);
            if let Some(&h1) = chars.peek() {
                hex.push(h1);
                chars.next();
            }
            if let Some(&h2) = chars.peek() {
                hex.push(h2);
                chars.next();
            }
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert arboard ImageData (RGBA) to PNG bytes
fn image_to_png(image: &arboard::ImageData) -> anyhow::Result<Vec<u8>> {
    use std::io::Cursor;
    let width = image.width as u32;
    let height = image.height as u32;
    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut png_data), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&image.bytes)?;
    }
    Ok(png_data)
}

/// Check if text is an image file path and upload it. Returns true if handled.
pub(crate) fn try_upload_image_file(
    text: &str,
    keys: &Keys,
    upload_tx: tokio::sync::mpsc::Sender<UploadResult>,
) -> bool {
    let path = text.trim();
    if path.is_empty() {
        return false;
    }

    let path = if let Some(file_path) = path.strip_prefix("file://") {
        urlencoded_decode(file_path)
    } else {
        path.replace("\\ ", " ")
    };

    let path_obj = std::path::Path::new(&path);
    let extension = match path_obj.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.to_lowercase(),
        None => return false,
    };

    let mime_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => return false,
    };

    if !path_obj.exists() {
        return false;
    }

    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(_) => return false,
    };

    let keys = keys.clone();
    let mime_type = mime_type.to_string();
    tokio::spawn(async move {
        let result = match tenex_core::nostr::upload_image(&data, &keys, &mime_type).await {
            Ok(url) => UploadResult::Success(url),
            Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
        };
        let _ = upload_tx.send(result).await;
    });

    true
}

/// Handle Ctrl+V clipboard paste — try image first, then text
pub(crate) fn handle_clipboard_paste(
    editor: &mut LineEditor,
    keys: &Keys,
    upload_tx: tokio::sync::mpsc::Sender<UploadResult>,
    stdout: &mut Stdout,
    state: &ReplState,
    runtime: &CoreRuntime,
    completion: &mut CompletionMenu,
    panel: &ConfigPanel,
    status_nav: &StatusBarNav,
    stats_panel: &StatsPanel,
    nudge_skill_panel: &NudgeSkillPanel,
) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_) => return,
    };

    if let Ok(image) = clipboard.get_image() {
        let png_data = match image_to_png(&image) {
            Ok(data) => data,
            Err(e) => {
                let msg = print_error_raw(&format!("Failed to convert image: {}", e));
                print_above_input(stdout, &msg, state, runtime, editor, completion, panel, status_nav, stats_panel, nudge_skill_panel);
                return;
            }
        };

        let msg = print_system_raw("Uploading image...");
        print_above_input(stdout, &msg, state, runtime, editor, completion, panel, status_nav, stats_panel, nudge_skill_panel);

        let keys = keys.clone();
        let tx = upload_tx;
        tokio::spawn(async move {
            let result = match tenex_core::nostr::upload_image(&png_data, &keys, "image/png").await
            {
                Ok(url) => UploadResult::Success(url),
                Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
            };
            let _ = tx.send(result).await;
        });
    } else if let Ok(text) = clipboard.get_text() {
        if !try_upload_image_file(&text, keys, upload_tx) {
            editor.handle_paste(&text);
            redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel, nudge_skill_panel);
        } else {
            let msg = print_system_raw("Uploading image...");
            print_above_input(stdout, &msg, state, runtime, editor, completion, panel, status_nav, stats_panel, nudge_skill_panel);
        }
    }
}

// ─── Bunker Command Handler ─────────────────────────────────────────────────

pub(crate) fn handle_bunker_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let arg = arg.unwrap_or("").trim();

    match arg {
        "" => {
            // Toggle bunker on/off
            if state.bunker_active {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = runtime.handle().send(NostrCommand::StopBunker { response_tx: tx });
                match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(Ok(())) => {
                        state.bunker_active = false;
                        vec![print_system_raw("Bunker stopped")]
                    }
                    Ok(Err(e)) => vec![print_error_raw(&format!("Failed to stop bunker: {e}"))],
                    Err(_) => vec![print_error_raw("Bunker stop timed out")],
                }
            } else {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = runtime.handle().send(NostrCommand::StartBunker { response_tx: tx });
                match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                    Ok(Ok(uri)) => {
                        state.bunker_active = true;
                        vec![
                            print_system_raw("Bunker started"),
                            format!("{DIM}{uri}{RESET}"),
                        ]
                    }
                    Ok(Err(e)) => vec![print_error_raw(&format!("Failed to start bunker: {e}"))],
                    Err(_) => vec![print_error_raw("Bunker start timed out")],
                }
            }
        }
        "audit" => {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = runtime.handle().send(NostrCommand::GetBunkerAuditLog { response_tx: tx });
            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(entries) => {
                    if entries.is_empty() {
                        return vec![print_system_raw("No audit log entries")];
                    }
                    let mut output = vec![format!("{WHITE_BOLD}Bunker Audit Log:{RESET}")];
                    for entry in entries.iter().rev().take(20) {
                        let ts_secs = entry.timestamp_ms / 1000;
                        let dt = chrono::DateTime::from_timestamp(ts_secs as i64, 0)
                            .map(|d| d.format("%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| "?".to_string());
                        let short_pk = if entry.requester_pubkey.len() >= 8 {
                            &entry.requester_pubkey[..8]
                        } else {
                            &entry.requester_pubkey
                        };
                        let kind_str = entry.event_kind.map(|k| format!("k:{k}")).unwrap_or_default();
                        output.push(format!(
                            "  {DIM}{dt}{RESET}  {short_pk}…  {kind_str}  {GREEN}{}{RESET}  {DIM}{}ms{RESET}",
                            entry.decision, entry.response_time_ms
                        ));
                    }
                    output
                }
                Err(_) => vec![print_error_raw("Failed to fetch audit log")],
            }
        }
        _ if arg.starts_with("rules remove") => {
            let idx_str = arg.strip_prefix("rules remove").unwrap().trim();
            let idx: usize = match idx_str.parse() {
                Ok(n) if n > 0 => n,
                _ => return vec![print_error_raw("Usage: /bunker rules remove <N>")],
            };

            let (tx, rx) = std::sync::mpsc::channel();
            let _ = runtime.handle().send(NostrCommand::GetBunkerAutoApproveRules { response_tx: tx });
            let rules = match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(r) => r,
                Err(_) => return vec![print_error_raw("Failed to fetch rules")],
            };

            let Some(rule) = rules.get(idx - 1) else {
                return vec![print_error_raw(&format!("No rule at index {idx}"))];
            };

            let _ = runtime.handle().send(NostrCommand::RemoveBunkerAutoApproveRule {
                requester_pubkey: rule.requester_pubkey.clone(),
                event_kind: rule.event_kind,
            });

            vec![print_system_raw(&format!("Removed rule #{idx}"))]
        }
        "rules" => {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = runtime.handle().send(NostrCommand::GetBunkerAutoApproveRules { response_tx: tx });
            match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(rules) => {
                    if rules.is_empty() {
                        return vec![print_system_raw("No auto-approve rules")];
                    }
                    let mut output = vec![format!("{WHITE_BOLD}Auto-approve Rules:{RESET}")];
                    for (i, rule) in rules.iter().enumerate() {
                        let short_pk = if rule.requester_pubkey.len() >= 12 {
                            &rule.requester_pubkey[..12]
                        } else {
                            &rule.requester_pubkey
                        };
                        let kind_str = rule.event_kind
                            .map(|k| format!("kind:{k}"))
                            .unwrap_or_else(|| "any kind".to_string());
                        output.push(format!("  {}: {short_pk}…  {kind_str}", i + 1));
                    }
                    output
                }
                Err(_) => vec![print_error_raw("Failed to fetch rules")],
            }
        }
        _ => vec![print_error_raw("Usage: /bunker [audit|rules|rules remove <N>]")],
    }
}
