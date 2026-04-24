use crate::util::{format_day_label, format_runtime};
use crate::{CYAN, DIM, GREEN, RESET, WHITE_BOLD};
use std::collections::HashSet;
use tenex_core::nostr::NostrCommand;
use tenex_core::runtime::CoreRuntime;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PanelMode {
    AgentSelect,
    FlagSelect,
    ModelSelect,
}

/// REPL agent-config panel.
///
/// Per the current protocol the `/config` command targets an agent's global
/// `default` config (kind:24020). It can pick a model, toggle PM, and always
/// sends the agent's full `skills`/`mcp_servers` snapshot from the agent's
/// latest kind:24011 event so the backend replaces rather than clears those
/// sets. `tool` tags were dropped from the protocol entirely.
pub(crate) struct ConfigPanel {
    pub(crate) active: bool,
    pub(crate) mode: PanelMode,
    pub(crate) agent_pubkey: String,
    pub(crate) agent_name: String,
    pub(crate) project_a_tag: String,
    pub(crate) items: Vec<String>,
    pub(crate) cursor: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) origin_command: String,

    pub(crate) pending_model: Option<String>,
    pub(crate) is_set_pm: bool,
    pub(crate) filter: String,
    pub(crate) quick_save: bool,
}

impl ConfigPanel {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            mode: PanelMode::FlagSelect,
            agent_pubkey: String::new(),
            agent_name: String::new(),
            project_a_tag: String::new(),
            items: Vec::new(),
            cursor: 0,
            scroll_offset: 0,
            origin_command: String::new(),
            pending_model: None,
            is_set_pm: false,
            filter: String::new(),
            quick_save: false,
        }
    }

    pub(crate) fn switch_to_agent_select(&mut self, runtime: &CoreRuntime) {
        self.mode = PanelMode::AgentSelect;
        self.filter.clear();
        self.cursor = 0;
        self.scroll_offset = 0;

        let store = runtime.data_store();
        let store_ref = store.borrow();
        let mut agent_items = Vec::new();
        if let Some(status) = store_ref.get_project_status(&self.project_a_tag) {
            let backend = status.backend_pubkey.clone();
            for agent in &status.agents {
                let model = store_ref
                    .get_agent_config(&backend, &agent.pubkey)
                    .and_then(|c| c.active_model.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let pm = if agent.is_pm { " [PM]" } else { "" };
                agent_items.push(format!("{}{pm} ({model})", agent.name));
            }
        }
        self.items = agent_items;
    }

    pub(crate) fn switch_to_flag_select(&mut self) {
        self.mode = PanelMode::FlagSelect;
        self.cursor = 0;
        self.scroll_offset = 0;
        self.filter.clear();
        self.rebuild_flag_items();
    }

    pub(crate) fn rebuild_flag_items(&mut self) {
        let pm_marker = if self.is_set_pm { "[x]" } else { "[ ]" };
        self.items = vec![format!("→  --model"), format!("{pm_marker} --set-pm")];
    }

    pub(crate) fn switch_to_model_select(&mut self, runtime: &CoreRuntime) {
        self.mode = PanelMode::ModelSelect;
        self.filter.clear();
        self.cursor = 0;
        self.scroll_offset = 0;

        let store = runtime.data_store();
        let store_ref = store.borrow();
        let backend = store_ref
            .get_project_status(&self.project_a_tag)
            .map(|s| s.backend_pubkey.clone());
        self.items = backend
            .as_deref()
            .and_then(|bp| store_ref.get_agent_config(bp, &self.agent_pubkey))
            .map(|c| c.models.clone())
            .unwrap_or_default();
    }

    /// Returns filtered items as (original_index, &item) pairs.
    pub(crate) fn filtered_items(&self) -> Vec<(usize, &String)> {
        if self.filter.is_empty() || matches!(self.mode, PanelMode::FlagSelect) {
            return self.items.iter().enumerate().collect();
        }
        let lower = self.filter.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.to_lowercase().contains(&lower))
            .collect()
    }

    pub(crate) fn rebuild_origin_command(&mut self) {
        let mut parts = vec!["/config".to_string()];
        if !self.agent_name.is_empty() {
            parts.push(format!("@{}", self.agent_name));
        }
        if self.is_set_pm {
            parts.push("--set-pm".to_string());
        }
        if let Some(ref model) = self.pending_model {
            parts.push(format!("--model {model}"));
        }
        self.origin_command = parts.join(" ");
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
        let count = self.filtered_items().len();
        if count > 0 && self.cursor < count - 1 {
            self.cursor += 1;
            if self.cursor >= self.scroll_offset + 15 {
                self.scroll_offset = self.cursor.saturating_sub(14);
            }
        }
    }

    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.items.clear();
        self.pending_model = None;
        self.is_set_pm = false;
        self.filter.clear();
        self.quick_save = false;
        self.cursor = 0;
        self.scroll_offset = 0;
    }

    /// Publish a kind:24020 update for this agent.
    ///
    /// The skill/mcp snapshots come from the cached kind:24011 event so the
    /// backend replaces — rather than clears — those sets when only the model
    /// or PM flag changed.
    pub(crate) fn save(&self, runtime: &CoreRuntime) -> String {
        let store = runtime.data_store();
        let store_ref = store.borrow();

        let Some(status) = store_ref.get_project_status(&self.project_a_tag) else {
            return format!("Project {} is offline", self.project_a_tag);
        };

        let Some(agent) = status
            .agents
            .iter()
            .find(|a| a.pubkey == self.agent_pubkey)
        else {
            return format!(
                "Agent {} no longer found in project status",
                self.agent_name
            );
        };

        let config = store_ref.get_agent_config(&status.backend_pubkey, &agent.pubkey);
        let model = self
            .pending_model
            .clone()
            .or_else(|| config.and_then(|c| c.active_model.clone()));
        let skills = config
            .map(|c| c.active_skills.clone())
            .unwrap_or_default();
        let mcp_servers = config.map(|c| c.active_mcps.clone()).unwrap_or_default();

        let tags: Vec<String> = if self.is_set_pm || agent.is_pm {
            vec!["pm".to_string()]
        } else {
            vec![]
        };

        drop(store_ref);

        let _ = runtime.handle().send(NostrCommand::UpdateAgentConfig {
            agent_pubkey: self.agent_pubkey.clone(),
            model,
            skills,
            mcp_servers,
            tags,
        });

        let mut changes = Vec::new();
        if self.pending_model.is_some() {
            changes.push(format!(
                "model → {}",
                self.pending_model.as_deref().unwrap_or("?")
            ));
        }
        if self.is_set_pm {
            changes.push("set as PM".to_string());
        }

        if changes.is_empty() {
            format!("No changes for {}", self.agent_name)
        } else {
            format!("Updated {} [{}]", self.agent_name, changes.join(", "))
        }
    }

    /// Resolve the selected agent from AgentSelect mode.
    pub(crate) fn resolve_selected_agent(&mut self, runtime: &CoreRuntime) -> bool {
        let filtered = self.filtered_items();
        let Some(&(orig_idx, _)) = filtered.get(self.cursor) else {
            return false;
        };

        let store = runtime.data_store();
        let store_ref = store.borrow();
        let Some(status) = store_ref.get_project_status(&self.project_a_tag) else {
            return false;
        };
        let Some(agent) = status.agents.get(orig_idx) else {
            return false;
        };

        self.agent_pubkey = agent.pubkey.clone();
        self.agent_name = agent.name.clone();

        self.pending_model = None;
        self.is_set_pm = agent.is_pm;

        true
    }

    /// Select current model from ModelSelect filtered list.
    pub(crate) fn select_current_model(&mut self) -> bool {
        let filtered = self.filtered_items();
        let Some(&(orig_idx, _)) = filtered.get(self.cursor) else {
            return false;
        };
        if let Some(model) = self.items.get(orig_idx) {
            self.pending_model = Some(model.clone());
            true
        } else {
            false
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
    OpenConversation {
        thread_id: String,
        project_a_tag: Option<String>,
    },
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
                    lines.push(format!(
                        "  {WHITE_BOLD}{:<40} {:>10}{RESET}",
                        "Project", "Cost"
                    ));
                    widths.push(54);
                    lines.push(format!("  {DIM}{}{RESET}", "─".repeat(52)));
                    widths.push(54);
                    for (_a_tag, name, cost) in &costs {
                        let display_name = if name.len() > 38 {
                            format!("{}…", &name[..37])
                        } else {
                            name.clone()
                        };
                        lines.push(format!(
                            "  {:<40} {GREEN}${:>8.2}{RESET}",
                            display_name, cost
                        ));
                        widths.push(54);
                    }
                    let total: f64 = costs.iter().map(|(_, _, c)| c).sum();
                    lines.push(format!("  {DIM}{}{RESET}", "─".repeat(52)));
                    widths.push(54);
                    lines.push(format!(
                        "  {WHITE_BOLD}{:<40} ${:>8.2}{RESET}",
                        "Total", total
                    ));
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
                        let filled = if max_ms > 0 {
                            (*ms as f64 / max_ms as f64 * bar_width as f64) as usize
                        } else {
                            0
                        };
                        let bar =
                            format!("{}{}", "█".repeat(filled), "░".repeat(bar_width - filled));
                        lines.push(format!(
                            "  {DIM}{date}{RESET}  {GREEN}{bar}{RESET}  {runtime_str}"
                        ));
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
                        let filled = if max_count > 0 {
                            (*all_count as f64 / max_count as f64 * bar_width as f64) as usize
                        } else {
                            0
                        };
                        let bar =
                            format!("{}{}", "█".repeat(filled), "░".repeat(bar_width - filled));
                        lines.push(format!(
                            "  {DIM}{date}{RESET}  {CYAN}{bar}{RESET}  {count_str}"
                        ));
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
    pub(crate) depth: usize,
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

// ─── Skill Selector Panel ───────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum NudgeSkillMode {
    Skills,
}

/// Item in the skill selector: (id, title, description)
pub(crate) struct NudgeSkillItem {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) description: String,
}

pub(crate) struct NudgeSkillPanel {
    pub(crate) active: bool,
    pub(crate) mode: NudgeSkillMode,
    pub(crate) items: Vec<NudgeSkillItem>,
    pub(crate) cursor: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) filter: String,
    pub(crate) selected_ids: HashSet<String>,
}

impl NudgeSkillPanel {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            mode: NudgeSkillMode::Skills,
            items: Vec::new(),
            cursor: 0,
            scroll_offset: 0,
            filter: String::new(),
            selected_ids: HashSet::new(),
        }
    }

    pub(crate) fn activate(&mut self, runtime: &CoreRuntime, state_skill_ids: &[String]) {
        self.active = true;
        self.mode = NudgeSkillMode::Skills;
        self.filter.clear();
        self.cursor = 0;
        self.scroll_offset = 0;

        self.selected_ids.clear();
        for id in state_skill_ids {
            self.selected_ids.insert(id.clone());
        }

        self.load_items(runtime);
    }

    fn load_items(&mut self, runtime: &CoreRuntime) {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        self.items = store_ref
            .content
            .get_skills()
            .into_iter()
            .map(|s| NudgeSkillItem {
                id: s.id.clone(),
                title: s.title.clone(),
                description: s.description.clone(),
            })
            .collect();
    }

    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.items.clear();
        self.filter.clear();
        self.cursor = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn filtered_items(&self) -> Vec<(usize, &NudgeSkillItem)> {
        if self.filter.is_empty() {
            return self.items.iter().enumerate().collect();
        }
        let lower = self.filter.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.title.to_lowercase().contains(&lower)
                    || item.description.to_lowercase().contains(&lower)
            })
            .collect()
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
        let count = self.filtered_items().len();
        if count > 0 && self.cursor < count - 1 {
            self.cursor += 1;
            if self.cursor >= self.scroll_offset + 15 {
                self.scroll_offset = self.cursor.saturating_sub(14);
            }
        }
    }

    pub(crate) fn toggle_current(&mut self) {
        let id = {
            let filtered = self.filtered_items();
            filtered.get(self.cursor).map(|(_, item)| item.id.clone())
        };
        if let Some(id) = id {
            if self.selected_ids.contains(&id) {
                self.selected_ids.remove(&id);
            } else {
                self.selected_ids.insert(id);
            }
        }
    }

    /// Commit selections back to the current state vector.
    pub(crate) fn commit_selections(&self, runtime: &CoreRuntime) -> Vec<String> {
        let store = runtime.data_store();
        let store_ref = store.borrow();

        let skill_ids: HashSet<String> = store_ref
            .content
            .get_skills()
            .iter()
            .map(|s| s.id.clone())
            .collect();

        let mut selected_skills = Vec::new();

        for id in &self.selected_ids {
            if skill_ids.contains(id) {
                selected_skills.push(id.clone());
            }
        }

        selected_skills
    }
}

/// Tools whose q-tags should NOT be treated as delegations
pub(crate) const Q_TAG_DELEGATION_DENYLIST: &[&str] =
    &["mcp__tenex__lesson_learn", "mcp__tenex__lesson_get"];
