use std::collections::HashSet;
use std::io::{Stdout, Write};
use crate::{DIM, GREEN, WHITE_BOLD, CYAN, RED, RESET};
use crate::editor::LineEditor;
use crate::completion::CompletionMenu;
use crate::panels::{ConfigPanel, PanelMode, ConversationStackEntry, StatusBarNav, StatsPanel};
use crate::state::ReplState;
use crate::format::{format_message, print_separator_raw, print_user_message_raw, print_error_raw, print_system_raw, is_tool_use};
use crate::render::{print_above_input, redraw_input};
use crate::util::{thread_display_name, MESSAGES_TO_LOAD};
use tenex_core::runtime::CoreRuntime;
use tenex_core::nostr::NostrCommand;
use tenex_core::models::{Project, ProjectAgent, Thread};
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
        let lower = idx_str.to_lowercase();
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
            nudge_ids: vec![],
            skill_ids: vec![],
            reference_conversation_id: None,
            reference_report_a_tag: None,
            fork_message_id: None,
            response_tx: Some(response_tx),
        });

        if let Ok(event_id) = response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            state.current_conversation = Some(event_id);
            state.last_todo_items.clear();
        }

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
        nudge_ids: vec![],
        skill_ids: vec![],
        ask_author_pubkey: None,
        response_tx: None,
    });

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

    let mut is_model = false;
    let mut is_make_pm = false;
    let mut is_global = false;
    let mut agent_filter = String::new();

    for part in raw.split_whitespace() {
        match part {
            "--model" => is_model = true,
            "--make-pm" => is_make_pm = true,
            "--global" => is_global = true,
            _ => agent_filter = part.to_string(),
        }
    }

    let (agent_pubkey, agent_name) = match resolve_agent_for_config(&agent_filter, state, runtime, &a_tag) {
        Ok(pair) => pair,
        Err(msg) => return CommandResult::Lines(vec![print_error_raw(&msg)]),
    };

    if is_make_pm {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let agent = store_ref
            .get_project_status(&a_tag)
            .and_then(|s| s.agents.iter().find(|a| a.pubkey == agent_pubkey));
        let model = agent.and_then(|a| a.model.clone());
        let tools = agent.map(|a| a.tools.clone()).unwrap_or_default();
        drop(store_ref);

        if is_global {
            let _ = runtime.handle().send(NostrCommand::UpdateGlobalAgentConfig {
                agent_pubkey,
                model,
                tools,
                tags: vec!["pm".to_string()],
            });
        } else {
            let _ = runtime.handle().send(NostrCommand::UpdateAgentConfig {
                project_a_tag: a_tag,
                agent_pubkey,
                model,
                tools,
                tags: vec!["pm".to_string()],
            });
        }
        return CommandResult::Lines(vec![print_system_raw(&format!("Set {} as project manager", agent_name))]);
    }

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let status = store_ref.get_project_status(&a_tag);

    let agent = status.and_then(|s| s.agents.iter().find(|a| a.pubkey == agent_pubkey));

    if is_model {
        panel.mode = PanelMode::Model;
        panel.items = status.map(|s| s.models().iter().map(|m| m.to_string()).collect()).unwrap_or_default();
        panel.selected.clear();
        if let Some(model) = agent.and_then(|a| a.model.as_ref()) {
            panel.selected.insert(model.clone());
        }
    } else {
        panel.mode = PanelMode::Tools;
        panel.items = status.map(|s| s.all_tools().iter().map(|t| t.to_string()).collect()).unwrap_or_default();
        panel.selected = agent.map(|a| a.tools.iter().cloned().collect()).unwrap_or_default();
    }

    if panel.items.is_empty() {
        drop(store_ref);
        let what = if is_model { "models" } else { "tools" };
        return CommandResult::Lines(vec![print_error_raw(&format!("No {what} available for this project"))]);
    }

    panel.active = true;
    panel.agent_pubkey = agent_pubkey;
    panel.agent_name = agent_name;
    panel.project_a_tag = a_tag;
    panel.is_global = is_global;
    panel.cursor = 0;
    panel.scroll_offset = 0;
    panel.origin_command = format!("/config{}", if raw.is_empty() { String::new() } else { format!(" {raw}") });

    CommandResult::Lines(vec![])
}

pub(crate) fn handle_model_command(
    arg: Option<&str>,
    state: &mut ReplState,
    runtime: &CoreRuntime,
    panel: &mut ConfigPanel,
) -> CommandResult {
    let combined = match arg {
        Some(a) if !a.is_empty() => format!("--model {a}"),
        _ => "--model".to_string(),
    };
    handle_config_command(Some(&combined), state, runtime, panel)
}

// ─── Event Handlers ─────────────────────────────────────────────────────────

pub(crate) fn handle_core_event(event: &CoreEvent, state: &mut ReplState, runtime: &CoreRuntime) -> Option<String> {
    match event {
        CoreEvent::Message(msg) => {
            if msg.pubkey == state.user_pubkey {
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
                state.last_displayed_pubkey = Some(msg.pubkey.clone());
                return Some(format!("\n{}", print_separator_raw()));
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
                print_above_input(stdout, &msg, state, runtime, editor, completion, panel, status_nav, stats_panel);
                return;
            }
        };

        let msg = print_system_raw("Uploading image...");
        print_above_input(stdout, &msg, state, runtime, editor, completion, panel, status_nav, stats_panel);

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
            redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel);
        } else {
            let msg = print_system_raw("Uploading image...");
            print_above_input(stdout, &msg, state, runtime, editor, completion, panel, status_nav, stats_panel);
        }
    }
}
