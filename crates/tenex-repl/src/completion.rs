use std::collections::HashSet;
use crate::state::ReplState;
use crate::util::thread_display_name;
use tenex_core::models::{Project, ProjectAgent, Thread};
use tenex_core::runtime::CoreRuntime;
use tenex_core::store::app_data_store::AppDataStore;

pub(crate) const COMMANDS: &[(&str, &str)] = &[
    ("/project", "list or switch project"),
    ("/agent", "list or switch agent"),
    ("/new", "new context [agent@project]"),
    ("/conversations", "browse/open conversations"),
    ("/config", "configure agent tools/model"),
    ("/model", "change agent model"),
    ("/boot", "boot an offline project"),
    ("/active", "active work across all projects"),
    ("/stats", "usage statistics"),
    ("/status", "show current context"),
    ("/help", "show commands"),
    ("/quit", "exit"),
];

#[derive(Clone)]
pub(crate) enum ItemAction {
    ReplaceFull(String),
    Submit(String),
}

#[derive(Clone)]
pub(crate) struct CompletionItem {
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) action: ItemAction,
}

pub(crate) struct CompletionMenu {
    pub(crate) visible: bool,
    pub(crate) items: Vec<CompletionItem>,
    pub(crate) selected: usize,
    pub(crate) rendered_lines: u16,
    pub(crate) attachment_indicator_lines: u16,
    pub(crate) delegation_bar_lines: u16,
    pub(crate) input_wrap_lines: u16,
    pub(crate) cursor_row: u16,
    pub(crate) input_area_drawn: bool,
}

pub(crate) fn thread_completion_items(
    store: &AppDataStore,
    a_tag: &str,
    filter: &str,
    project_prefix: Option<&str>,
) -> Vec<CompletionItem> {
    let mut threads: Vec<&Thread> = store.get_threads(a_tag).iter().collect();
    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
    let threads: Vec<&Thread> = threads.into_iter().take(20).collect();
    threads
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            filter.is_empty()
                || t.title.to_lowercase().contains(filter)
                || t.summary
                    .as_ref()
                    .map(|s| s.to_lowercase().contains(filter))
                    .unwrap_or(false)
        })
        .map(|(i, t)| {
            let display = thread_display_name(t, 50);

            let mut desc_parts = Vec::new();
            let working = store.operations.get_working_agents(&t.id);
            if !working.is_empty() {
                let names: Vec<String> = working
                    .iter()
                    .map(|pk| store.get_profile_name(pk))
                    .collect();
                desc_parts.push(format!("⟡ {}", names.join(", ")));
            }
            if let Some(status) = &t.status_label {
                if !status.is_empty() {
                    desc_parts.push(status.clone());
                }
            }

            let value = match project_prefix {
                Some(proj) => format!("@{} {}", proj, i + 1),
                None => (i + 1).to_string(),
            };

            CompletionItem {
                label: display,
                description: desc_parts.join(" · "),
                action: ItemAction::Submit(value),
            }
        })
        .collect()
}

pub(crate) fn active_completion_items(store: &AppDataStore, filter: &str) -> Vec<CompletionItem> {
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut items: Vec<CompletionItem> = Vec::new();

    let active_ops = store.operations.get_all_active_operations();
    for op in &active_ops {
        let thread_id = op.thread_id.as_deref().unwrap_or(&op.event_id);
        if !seen_ids.insert(thread_id.to_string()) {
            continue;
        }
        let Some(thread) = store.get_thread_by_id(thread_id) else {
            continue;
        };
        let project_name = store
            .get_project_a_tag_for_thread(thread_id)
            .and_then(|a| {
                store
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == a)
                    .map(|p| p.title.clone())
            })
            .unwrap_or_default();

        let display = thread_display_name(thread, 45);

        if !filter.is_empty()
            && !display.to_lowercase().contains(filter)
            && !project_name.to_lowercase().contains(filter)
        {
            continue;
        }

        let agent_names: Vec<String> = op
            .agent_pubkeys
            .iter()
            .map(|pk| store.get_profile_name(pk))
            .collect();

        let idx = items.len() + 1;
        items.push(CompletionItem {
            label: display,
            description: format!("⟡ {} · {}", agent_names.join(", "), project_name),
            action: ItemAction::Submit(idx.to_string()),
        });
    }

    let mut recent_threads: Vec<(&Thread, String)> = Vec::new();
    for project in store.get_projects().iter().filter(|p| !p.is_deleted) {
        let a_tag = project.a_tag();
        for thread in store.get_threads(&a_tag) {
            if !seen_ids.contains(&thread.id) {
                recent_threads.push((thread, project.title.clone()));
            }
        }
    }
    recent_threads.sort_by(|a, b| b.0.effective_last_activity.cmp(&a.0.effective_last_activity));

    for (thread, project_name) in recent_threads.into_iter().take(15) {
        if !seen_ids.insert(thread.id.clone()) {
            continue;
        }

        let display = thread_display_name(thread, 45);

        if !filter.is_empty()
            && !display.to_lowercase().contains(filter)
            && !project_name.to_lowercase().contains(filter)
        {
            continue;
        }

        let mut desc_parts = Vec::new();
        let working = store.operations.get_working_agents(&thread.id);
        if !working.is_empty() {
            let names: Vec<String> = working
                .iter()
                .map(|pk| store.get_profile_name(pk))
                .collect();
            desc_parts.push(format!("⟡ {}", names.join(", ")));
        }
        if let Some(status) = &thread.status_label {
            if !status.is_empty() {
                desc_parts.push(status.clone());
            }
        }
        desc_parts.push(project_name);

        let idx = items.len() + 1;
        items.push(CompletionItem {
            label: display,
            description: desc_parts.join(" · "),
            action: ItemAction::Submit(idx.to_string()),
        });
    }

    items
}

pub(crate) fn agent_completion_items(
    store: &AppDataStore,
    a_tag: &str,
    filter: &str,
    project_prefix: Option<&str>,
    project_name: &str,
) -> Vec<CompletionItem> {
    let agents: Vec<&ProjectAgent> = store
        .get_online_agents(a_tag)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    agents
        .iter()
        .enumerate()
        .filter(|(_, a)| filter.is_empty() || a.name.to_lowercase().contains(filter))
        .map(|(i, a)| {
            let model = a.model.as_deref().unwrap_or("unknown");
            let pm = if a.is_pm { " [PM]" } else { "" };
            let value = match project_prefix {
                Some(proj) => format!("@{} {}", proj, i + 1),
                None => (i + 1).to_string(),
            };
            CompletionItem {
                label: format!("{}{pm}", a.name),
                description: format!("{model} · {project_name}"),
                action: ItemAction::Submit(value),
            }
        })
        .collect()
}

fn project_picker_items(runtime: &CoreRuntime, filter: &str, cmd: &str) -> Vec<CompletionItem> {
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let projects: Vec<&Project> = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .collect();
    let mut items: Vec<(bool, CompletionItem)> = projects
        .iter()
        .filter(|p| filter.is_empty() || p.title.to_lowercase().contains(filter))
        .map(|p| {
            let online = store_ref.is_project_online(&p.a_tag());
            let status = if online { "online" } else { "offline" };
            (online, CompletionItem {
                label: p.title.clone(),
                description: status.to_string(),
                action: ItemAction::ReplaceFull(format!("{cmd} @{} ", p.title)),
            })
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    items.into_iter().map(|(_, item)| item).collect()
}

impl CompletionMenu {
    pub(crate) fn new() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            selected: 0,
            rendered_lines: 0,
            attachment_indicator_lines: 0,
            delegation_bar_lines: 0,
            input_wrap_lines: 0,
            cursor_row: 0,
            input_area_drawn: false,
        }
    }

    pub(crate) fn update_from_buffer(&mut self, buffer: &str, state: &ReplState, runtime: &CoreRuntime) {
        if let Some(stripped) = buffer.strip_prefix('@') {
            let filter = stripped.to_lowercase();
            let store = runtime.data_store();
            let store_ref = store.borrow();
            let mut items = Vec::new();

            if let Some(ref current_a_tag) = state.current_project {
                let current_project_name = store_ref
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == *current_a_tag)
                    .map(|p| p.title.as_str())
                    .unwrap_or("unknown");
                items.extend(agent_completion_items(&store_ref, current_a_tag, &filter, None, current_project_name));
            }

            for project in store_ref.get_projects().iter().filter(|p| !p.is_deleted) {
                let a_tag = project.a_tag();
                if state.current_project.as_deref() == Some(&a_tag) {
                    continue;
                }
                let other_items = agent_completion_items(&store_ref, &a_tag, &filter, Some(&project.title), &project.title);
                items.extend(other_items);
            }

            self.items = items;
            self.selected = 0;
            self.visible = !self.items.is_empty();
            return;
        }

        if !buffer.starts_with('/') {
            self.hide();
            return;
        }

        let (cmd_part, arg_part) = match buffer.find(' ') {
            Some(pos) => (&buffer[..pos], Some(buffer[pos + 1..].trim_start())),
            None => (buffer, None),
        };

        match arg_part {
            None => {
                let lower = cmd_part.to_lowercase();
                self.items = COMMANDS
                    .iter()
                    .filter(|(cmd, _)| cmd.starts_with(&lower))
                    .map(|(cmd, desc)| CompletionItem {
                        label: cmd.to_string(),
                        description: desc.to_string(),
                        action: ItemAction::ReplaceFull(format!("{cmd} ")),
                    })
                    .collect();
            }
            Some(arg) => {
                let filter = arg.to_lowercase();
                match cmd_part {
                    "/project" | "/p" => {
                        let store = runtime.data_store();
                        let store_ref = store.borrow();
                        let projects: Vec<&Project> = store_ref
                            .get_projects()
                            .iter()
                            .filter(|p| !p.is_deleted)
                            .collect();
                        let mut items: Vec<(bool, CompletionItem)> = projects
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| filter.is_empty() || p.title.to_lowercase().contains(&filter))
                            .map(|(i, p)| {
                                let online = store_ref.is_project_online(&p.a_tag());
                                let status = if online { "online" } else { "offline" };
                                (online, CompletionItem {
                                    label: p.title.clone(),
                                    description: status.to_string(),
                                    action: ItemAction::Submit((i + 1).to_string()),
                                })
                            })
                            .collect();
                        items.sort_by(|a, b| b.0.cmp(&a.0));
                        self.items = items.into_iter().map(|(_, item)| item).collect();
                    }
                    "/agent" | "/a" => {
                        if let Some(at_pos) = arg.find('@') {
                            let after_at = &arg[at_pos + 1..];

                            if let Some(space_pos) = after_at.find(' ') {
                                let project_part = after_at[..space_pos].trim();
                                let agent_filter = after_at[space_pos + 1..].trim().to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let lower_proj = project_part.to_lowercase();
                                if let Some(project) = store_ref.get_projects().iter()
                                    .filter(|p| !p.is_deleted)
                                    .find(|p| p.title.to_lowercase().contains(&lower_proj))
                                {
                                    let a_tag = project.a_tag();
                                    self.items = agent_completion_items(&store_ref, &a_tag, &agent_filter, Some(project_part), &project.title);
                                }
                            } else {
                                let project_filter = after_at.to_lowercase();
                                self.items = project_picker_items(runtime, &project_filter, cmd_part);
                            }
                        } else if let Some(ref a_tag) = state.current_project {
                            let store = runtime.data_store();
                            let store_ref = store.borrow();
                            let proj_name = store_ref.get_projects().iter().find(|p| p.a_tag() == *a_tag).map(|p| p.title.as_str()).unwrap_or("unknown");
                            self.items = agent_completion_items(&store_ref, a_tag, &filter, None, proj_name);
                        } else {
                            self.items.clear();
                        }
                    }
                    "/open" | "/o" | "/conversations" | "/c" => {
                        if let Some(at_pos) = arg.find('@') {
                            let after_at = &arg[at_pos + 1..];

                            if let Some(space_pos) = after_at.find(' ') {
                                let project_part = after_at[..space_pos].trim();
                                let conv_filter = after_at[space_pos + 1..].trim().to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let lower_proj = project_part.to_lowercase();
                                if let Some(project) = store_ref.get_projects().iter()
                                    .filter(|p| !p.is_deleted)
                                    .find(|p| p.title.to_lowercase().contains(&lower_proj))
                                {
                                    let a_tag = project.a_tag();
                                    self.items = thread_completion_items(&store_ref, &a_tag, &conv_filter, Some(project_part));
                                }
                            } else {
                                let project_filter = after_at.to_lowercase();
                                self.items = project_picker_items(runtime, &project_filter, cmd_part);
                            }
                        } else if let Some(ref a_tag) = state.current_project {
                            let store = runtime.data_store();
                            let store_ref = store.borrow();
                            self.items = thread_completion_items(&store_ref, a_tag, &filter, None);
                        } else {
                            self.items.clear();
                        }
                    }
                    "/active" => {
                        let store = runtime.data_store();
                        let store_ref = store.borrow();
                        self.items = active_completion_items(&store_ref, &filter);
                    }
                    "/new" | "/n" => {
                        if let Some(at_pos) = arg.find('@') {
                            let agent_part = arg[..at_pos].trim();
                            let after_at = &arg[at_pos + 1..];

                            if let Some(space_pos) = after_at.find(' ') {
                                let project_part = after_at[..space_pos].trim();
                                let agent_filter = after_at[space_pos + 1..].trim().to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let projects: Vec<&Project> = store_ref
                                    .get_projects()
                                    .iter()
                                    .filter(|p| !p.is_deleted)
                                    .collect();
                                let lower_proj = project_part.to_lowercase();
                                if let Some(project) = projects.iter().find(|p| p.title.to_lowercase().contains(&lower_proj)) {
                                    let a_tag = project.a_tag();
                                    if let Some(agents) = store_ref.get_online_agents(&a_tag) {
                                        self.items = agents
                                            .iter()
                                            .filter(|a| agent_filter.is_empty() || a.name.to_lowercase().contains(&agent_filter))
                                            .map(|a| {
                                                let model = a.model.as_deref().unwrap_or("unknown");
                                                let pm = if a.is_pm { " [PM]" } else { "" };
                                                CompletionItem {
                                                    label: format!("{}{pm}", a.name),
                                                    description: model.to_string(),
                                                    action: ItemAction::Submit(format!("{}@{}", a.name, project_part)),
                                                }
                                            })
                                            .collect();
                                    }
                                }
                            } else if !agent_part.is_empty() {
                                let project_filter = after_at.to_lowercase();
                                let agent_lower = agent_part.to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let projects: Vec<&Project> = store_ref
                                    .get_projects()
                                    .iter()
                                    .filter(|p| !p.is_deleted)
                                    .collect();

                                let mut items: Vec<(bool, CompletionItem)> = Vec::new();
                                for project in &projects {
                                    let a_tag = project.a_tag();
                                    if !project_filter.is_empty() && !project.title.to_lowercase().contains(&project_filter) {
                                        continue;
                                    }
                                    let has_agent = store_ref
                                        .get_online_agents(&a_tag)
                                        .map(|agents| agents.iter().any(|a| a.name.to_lowercase().contains(&agent_lower)))
                                        .unwrap_or(false);
                                    if !has_agent {
                                        continue;
                                    }
                                    let online = store_ref.is_project_online(&a_tag);
                                    let status = if online { "online" } else { "offline" };
                                    items.push((online, CompletionItem {
                                        label: project.title.clone(),
                                        description: status.to_string(),
                                        action: ItemAction::Submit(format!("{}@{}", agent_part, project.title)),
                                    }));
                                }
                                items.sort_by(|a, b| b.0.cmp(&a.0));
                                self.items = items.into_iter().map(|(_, item)| item).collect();
                            } else {
                                let project_filter = after_at.to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let projects: Vec<&Project> = store_ref
                                    .get_projects()
                                    .iter()
                                    .filter(|p| !p.is_deleted)
                                    .collect();

                                let mut items: Vec<(bool, CompletionItem)> = projects
                                    .iter()
                                    .filter(|p| project_filter.is_empty() || p.title.to_lowercase().contains(&project_filter))
                                    .map(|p| {
                                        let online = store_ref.is_project_online(&p.a_tag());
                                        let status = if online { "online" } else { "offline" };
                                        (online, CompletionItem {
                                            label: p.title.clone(),
                                            description: status.to_string(),
                                            action: ItemAction::ReplaceFull(format!("/new @{} ", p.title)),
                                        })
                                    })
                                    .collect();
                                items.sort_by(|a, b| b.0.cmp(&a.0));
                                self.items = items.into_iter().map(|(_, item)| item).collect();
                            }
                        } else if let Some(ref a_tag) = state.current_project {
                            let store = runtime.data_store();
                            let store_ref = store.borrow();
                            let agents: Vec<&ProjectAgent> = store_ref
                                .get_online_agents(a_tag)
                                .map(|a| a.iter().collect())
                                .unwrap_or_default();
                            self.items = agents
                                .iter()
                                .filter(|a| filter.is_empty() || a.name.to_lowercase().contains(&filter))
                                .map(|a| {
                                    let model = a.model.as_deref().unwrap_or("unknown");
                                    let pm = if a.is_pm { " [PM]" } else { "" };
                                    CompletionItem {
                                        label: format!("{}{pm}", a.name),
                                        description: model.to_string(),
                                        action: ItemAction::ReplaceFull(format!("/new {}", a.name)),
                                    }
                                })
                                .collect();
                        } else {
                            self.items.clear();
                        }
                    }
                    "/boot" | "/b" => {
                        let store = runtime.data_store();
                        let store_ref = store.borrow();
                        let projects: Vec<&Project> = store_ref
                            .get_projects()
                            .iter()
                            .filter(|p| !p.is_deleted)
                            .collect();
                        self.items = projects
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| {
                                let online = store_ref.is_project_online(&p.a_tag());
                                !online && (filter.is_empty() || p.title.to_lowercase().contains(&filter))
                            })
                            .map(|(i, p)| CompletionItem {
                                label: p.title.clone(),
                                description: "offline".to_string(),
                                action: ItemAction::Submit((i + 1).to_string()),
                            })
                            .collect();
                    }
                    "/config" => {
                        let parts: Vec<&str> = arg.splitn(2, ' ').collect();
                        let first = parts[0];
                        let rest = parts.get(1).map(|s| s.trim()).unwrap_or("");

                        if first.starts_with("--") || first.is_empty() {
                            let flag_filter = if first.starts_with("--") && rest.is_empty() && !first.contains(' ') {
                                first
                            } else {
                                ""
                            };
                            let agent_filter = if first.starts_with("--") { rest } else { first };

                            let mut items = Vec::new();

                            if !first.starts_with("--") || (first.starts_with("--") && rest.is_empty()) {
                                let flags = [
                                    ("--model", "change model"),
                                    ("--make-pm", "set as project manager"),
                                    ("--global", "apply globally"),
                                ];
                                for (flag, desc) in &flags {
                                    if flag_filter.is_empty() || flag.starts_with(flag_filter) {
                                        items.push(CompletionItem {
                                            label: flag.to_string(),
                                            description: desc.to_string(),
                                            action: ItemAction::ReplaceFull(format!("/config {flag} ")),
                                        });
                                    }
                                }
                            }

                            if let Some(ref a_tag) = state.current_project {
                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let agent_filter_lower = agent_filter.to_lowercase();
                                let proj_name = store_ref.get_projects().iter().find(|p| p.a_tag() == *a_tag).map(|p| p.title.as_str()).unwrap_or("unknown");
                                items.extend(agent_completion_items(&store_ref, a_tag, &agent_filter_lower, None, proj_name));
                            }

                            self.items = items;
                        } else if let Some(ref a_tag) = state.current_project {
                            let store = runtime.data_store();
                            let store_ref = store.borrow();
                            let proj_name = store_ref.get_projects().iter().find(|p| p.a_tag() == *a_tag).map(|p| p.title.as_str()).unwrap_or("unknown");
                            self.items = agent_completion_items(&store_ref, a_tag, &filter, None, proj_name);
                        } else {
                            self.items.clear();
                        }
                    }
                    "/model" | "/m" => {
                        if let Some(ref a_tag) = state.current_project {
                            let store = runtime.data_store();
                            let store_ref = store.borrow();
                            let proj_name = store_ref.get_projects().iter().find(|p| p.a_tag() == *a_tag).map(|p| p.title.as_str()).unwrap_or("unknown");
                            self.items = agent_completion_items(&store_ref, a_tag, &filter, None, proj_name);
                        } else {
                            self.items.clear();
                        }
                    }
                    _ => {
                        self.items.clear();
                    }
                }
            }
        }

        self.visible = !self.items.is_empty();
        self.selected = 0;
    }

    pub(crate) fn hide(&mut self) {
        self.visible = false;
        self.items.clear();
        self.selected = 0;
    }

    pub(crate) fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    pub(crate) fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub(crate) fn accept(&mut self) -> Option<ItemAction> {
        if !self.visible || self.items.is_empty() {
            return None;
        }
        let action = self.items[self.selected].action.clone();
        self.hide();
        Some(action)
    }
}
