use std::collections::HashSet;
use crate::{CYAN, GREEN, WHITE_BOLD, DIM, RESET};
use crate::util::{format_day_label, format_runtime};
use tenex_core::nostr::NostrCommand;
use tenex_core::runtime::CoreRuntime;

pub(crate) enum PanelMode {
    Tools,
    Model,
}

pub(crate) struct ConfigPanel {
    pub(crate) active: bool,
    pub(crate) mode: PanelMode,
    pub(crate) agent_pubkey: String,
    pub(crate) agent_name: String,
    pub(crate) project_a_tag: String,
    pub(crate) is_global: bool,
    pub(crate) items: Vec<String>,
    pub(crate) selected: HashSet<String>,
    pub(crate) cursor: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) origin_command: String,
}

impl ConfigPanel {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            mode: PanelMode::Tools,
            agent_pubkey: String::new(),
            agent_name: String::new(),
            project_a_tag: String::new(),
            is_global: false,
            items: Vec::new(),
            selected: HashSet::new(),
            cursor: 0,
            scroll_offset: 0,
            origin_command: String::new(),
        }
    }

    pub(crate) fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            if self.cursor < self.scroll_offset {
                self.scroll_offset = self.cursor;
            }
        }
    }

    pub(crate) fn move_down(&mut self) {
        if !self.items.is_empty() && self.cursor < self.items.len() - 1 {
            self.cursor += 1;
            if self.cursor >= self.scroll_offset + 15 {
                self.scroll_offset = self.cursor.saturating_sub(14);
            }
        }
    }

    pub(crate) fn toggle_current(&mut self) {
        if let Some(item) = self.items.get(self.cursor) {
            let item = item.clone();
            if self.selected.contains(&item) {
                self.selected.remove(&item);
            } else {
                self.selected.insert(item);
            }
        }
    }

    pub(crate) fn select_current(&mut self) {
        if let Some(item) = self.items.get(self.cursor) {
            self.selected.clear();
            self.selected.insert(item.clone());
        }
    }

    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.items.clear();
        self.selected.clear();
        self.cursor = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn save(&self, runtime: &CoreRuntime) -> String {
        let store = runtime.data_store();
        let store_ref = store.borrow();

        let agent = match store_ref
            .get_project_status(&self.project_a_tag)
            .and_then(|s| s.agents.iter().find(|a| a.pubkey == self.agent_pubkey))
        {
            Some(a) => a,
            None => {
                return format!("Agent {} no longer found in project status", self.agent_name);
            }
        };

        let (model, tools, tags) = match self.mode {
            PanelMode::Tools => {
                let model = agent.model.clone();
                let tools: Vec<String> = self.selected.iter().cloned().collect();
                let tags: Vec<String> = if agent.is_pm {
                    vec!["pm".to_string()]
                } else {
                    vec![]
                };
                (model, tools, tags)
            }
            PanelMode::Model => {
                let model = self.selected.iter().next().cloned();
                let tools = agent.tools.clone();
                let tags: Vec<String> = if agent.is_pm {
                    vec!["pm".to_string()]
                } else {
                    vec![]
                };
                (model, tools, tags)
            }
        };
        drop(store_ref);

        if self.is_global {
            let _ = runtime.handle().send(NostrCommand::UpdateGlobalAgentConfig {
                agent_pubkey: self.agent_pubkey.clone(),
                model,
                tools,
                tags,
            });
        } else {
            let _ = runtime.handle().send(NostrCommand::UpdateAgentConfig {
                project_a_tag: self.project_a_tag.clone(),
                agent_pubkey: self.agent_pubkey.clone(),
                model,
                tools,
                tags,
            });
        }

        match self.mode {
            PanelMode::Tools => format!(
                "Updated tools for {} ({})",
                self.agent_name,
                if self.is_global { "global" } else { "project" }
            ),
            PanelMode::Model => {
                let model_name = self.selected.iter().next().map(|s| s.as_str()).unwrap_or("none");
                format!("Set model for {} → {}", self.agent_name, model_name)
            }
        }
    }
}

pub(crate) struct StatusBarNav {
    pub(crate) active: bool,
    pub(crate) segment: usize,
    pub(crate) segment_count: usize,
}

impl StatusBarNav {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            segment: 0,
            segment_count: 0,
        }
    }

    pub(crate) fn activate(&mut self) {
        self.active = true;
        self.segment = 0;
    }

    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.segment = 0;
    }

    pub(crate) fn move_left(&mut self) {
        if self.segment > 0 {
            self.segment -= 1;
        }
    }

    pub(crate) fn move_right(&mut self) {
        if self.segment + 1 < self.segment_count {
            self.segment += 1;
        }
    }
}

pub(crate) enum StatusBarAction {
    ShowCompletion(String),
    OpenConversation { thread_id: String, project_a_tag: Option<String> },
    OpenStats,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum StatsTab {
    Rankings,
    Runtime,
    Messages,
}

impl StatsTab {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Rankings => Self::Runtime,
            Self::Runtime => Self::Messages,
            Self::Messages => Self::Rankings,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Rankings => Self::Messages,
            Self::Runtime => Self::Rankings,
            Self::Messages => Self::Runtime,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rankings => "Rankings",
            Self::Runtime => "Runtime",
            Self::Messages => "Messages",
        }
    }
}

pub(crate) struct StatsPanel {
    pub(crate) active: bool,
    pub(crate) tab: StatsTab,
    pub(crate) scroll_offset: usize,
    pub(crate) content_lines: Vec<String>,
    pub(crate) content_plain_widths: Vec<usize>,
    pub(crate) total_lines: usize,
}

impl StatsPanel {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            tab: StatsTab::Rankings,
            scroll_offset: 0,
            content_lines: Vec::new(),
            content_plain_widths: Vec::new(),
            total_lines: 0,
        }
    }

    pub(crate) fn activate(&mut self, runtime: &CoreRuntime) {
        self.active = true;
        self.tab = StatsTab::Rankings;
        self.scroll_offset = 0;
        self.render_content(runtime);
    }

    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.content_lines.clear();
        self.content_plain_widths.clear();
    }

    pub(crate) fn switch_tab(&mut self, tab: StatsTab, runtime: &CoreRuntime) {
        self.tab = tab;
        self.scroll_offset = 0;
        self.render_content(runtime);
    }

    pub(crate) fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub(crate) fn scroll_down(&mut self) {
        let max_visible = 16;
        if self.total_lines > max_visible {
            self.scroll_offset = self.scroll_offset.min(self.total_lines - max_visible);
            if self.scroll_offset < self.total_lines.saturating_sub(max_visible) {
                self.scroll_offset += 1;
            }
        }
    }

    fn render_content(&mut self, runtime: &CoreRuntime) {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let mut lines = Vec::new();
        let mut widths = Vec::new();

        match self.tab {
            StatsTab::Rankings => {
                let costs = store_ref.get_cost_by_project();
                if costs.is_empty() {
                    lines.push(format!("  {DIM}No cost data yet{RESET}"));
                    widths.push(18);
                } else {
                    lines.push(format!("  {WHITE_BOLD}{:<40} {:>10}{RESET}", "Project", "Cost"));
                    widths.push(54);
                    lines.push(format!("  {DIM}{}{RESET}", "─".repeat(52)));
                    widths.push(54);
                    for (_a_tag, name, cost) in &costs {
                        let display_name = if name.len() > 38 {
                            format!("{}…", &name[..37])
                        } else {
                            name.clone()
                        };
                        lines.push(format!("  {:<40} {GREEN}${:>8.2}{RESET}", display_name, cost));
                        widths.push(54);
                    }
                    let total: f64 = costs.iter().map(|(_, _, c)| c).sum();
                    lines.push(format!("  {DIM}{}{RESET}", "─".repeat(52)));
                    widths.push(54);
                    lines.push(format!("  {WHITE_BOLD}{:<40} ${:>8.2}{RESET}", "Total", total));
                    widths.push(54);
                }
            }
            StatsTab::Runtime => {
                let data = store_ref.statistics.get_runtime_by_day(14);
                if data.is_empty() {
                    lines.push(format!("  {DIM}No runtime data yet{RESET}"));
                    widths.push(21);
                } else {
                    let max_ms = data.iter().map(|(_, ms)| *ms).max().unwrap_or(1).max(1);
                    let bar_width = 30;
                    for (ts, ms) in &data {
                        let date = format_day_label(*ts);
                        let runtime_str = format_runtime(*ms);
                        let filled = if max_ms > 0 { (*ms as f64 / max_ms as f64 * bar_width as f64) as usize } else { 0 };
                        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(bar_width - filled));
                        lines.push(format!("  {DIM}{date}{RESET}  {GREEN}{bar}{RESET}  {runtime_str}"));
                        widths.push(2 + date.len() + 2 + bar_width + 2 + runtime_str.len());
                    }
                }
            }
            StatsTab::Messages => {
                let (user_data, all_data) = store_ref.get_messages_by_day(14);
                if all_data.is_empty() {
                    lines.push(format!("  {DIM}No message data yet{RESET}"));
                    widths.push(21);
                } else {
                    let max_count = all_data.iter().map(|(_, c)| *c).max().unwrap_or(1).max(1);
                    let bar_width = 30;
                    for (i, (ts, all_count)) in all_data.iter().enumerate() {
                        let user_count = user_data.get(i).map(|(_, c)| *c).unwrap_or(0);
                        let date = format_day_label(*ts);
                        let count_str = format!("{}/{}", user_count, all_count);
                        let filled = if max_count > 0 { (*all_count as f64 / max_count as f64 * bar_width as f64) as usize } else { 0 };
                        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(bar_width - filled));
                        lines.push(format!("  {DIM}{date}{RESET}  {CYAN}{bar}{RESET}  {count_str}"));
                        widths.push(2 + date.len() + 2 + bar_width + 2 + count_str.len());
                    }
                }
            }
        }

        self.total_lines = lines.len();
        self.content_lines = lines;
        self.content_plain_widths = widths;
    }
}

pub(crate) struct ConversationStackEntry {
    pub(crate) thread_id: String,
    pub(crate) project_a_tag: Option<String>,
}

pub(crate) struct DelegationEntry {
    pub(crate) thread_id: String,
    pub(crate) label: String,
    pub(crate) is_busy: bool,
    pub(crate) is_parent: bool,
    pub(crate) current_activity: Option<String>,
    pub(crate) todo_summary: Option<(String, bool)>,
}

pub(crate) struct DelegationBar {
    pub(crate) focused: bool,
    pub(crate) selected: usize,
    pub(crate) visible_delegations: Vec<DelegationEntry>,
}

impl DelegationBar {
    pub(crate) fn new() -> Self {
        Self {
            focused: false,
            selected: 0,
            visible_delegations: Vec::new(),
        }
    }

    pub(crate) fn has_content(&self) -> bool {
        !self.visible_delegations.is_empty()
    }

    pub(crate) fn focus(&mut self) {
        self.focused = true;
        if self.selected >= self.visible_delegations.len() {
            self.selected = 0;
        }
    }

    pub(crate) fn unfocus(&mut self) {
        self.focused = false;
    }

    pub(crate) fn select_next(&mut self) {
        if !self.visible_delegations.is_empty() {
            self.selected = (self.selected + 1) % self.visible_delegations.len();
        }
    }

    pub(crate) fn select_prev(&mut self) {
        if !self.visible_delegations.is_empty() {
            self.selected = if self.selected == 0 {
                self.visible_delegations.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub(crate) fn selected_entry(&self) -> Option<&DelegationEntry> {
        self.visible_delegations.get(self.selected)
    }
}

/// Tools whose q-tags should NOT be treated as delegations
pub(crate) const Q_TAG_DELEGATION_DENYLIST: &[&str] = &[
    "mcp__tenex__report_write",
    "mcp__tenex__report_read",
    "mcp__tenex__report_delete",
    "mcp__tenex__lesson_learn",
    "mcp__tenex__lesson_get",
];
