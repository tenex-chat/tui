use std::time::Instant;
use crate::{CYAN, GREEN, DIM, YELLOW, RESET};
use crate::panels::{DelegationBar, ConversationStackEntry, StatusBarAction};
use crate::util::{strip_ansi, thread_display_name, wave_colorize, ANIMATION_DURATION_MS, ANIMATION_DURATION_F64};
use tenex_core::runtime::CoreRuntime;

pub(crate) struct ReplState {
    pub(crate) current_project: Option<String>,
    pub(crate) current_agent: Option<String>,
    pub(crate) current_agent_name: Option<String>,
    pub(crate) current_conversation: Option<String>,
    pub(crate) user_pubkey: String,
    pub(crate) streaming_in_progress: bool,
    pub(crate) stream_buffer: String,
    pub(crate) last_displayed_pubkey: Option<String>,
    pub(crate) project_anim_start: Option<Instant>,
    pub(crate) project_anim_name: String,
    pub(crate) wave_frame: u64,
    pub(crate) conversation_stack: Vec<ConversationStackEntry>,
    pub(crate) delegation_bar: DelegationBar,
    pub(crate) last_todo_items: Vec<(String, String)>,
}

impl ReplState {
    pub(crate) fn new(user_pubkey: String) -> Self {
        Self {
            current_project: None,
            current_agent: None,
            current_agent_name: None,
            current_conversation: None,
            user_pubkey,
            streaming_in_progress: false,
            stream_buffer: String::new(),
            last_displayed_pubkey: None,
            project_anim_start: None,
            project_anim_name: String::new(),
            wave_frame: 0,
            conversation_stack: Vec::new(),
            delegation_bar: DelegationBar::new(),
            last_todo_items: Vec::new(),
        }
    }

    pub(crate) fn start_project_animation(&mut self, name: &str) {
        self.project_anim_start = Some(Instant::now());
        self.project_anim_name = name.to_string();
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.project_anim_start
            .map(|t| t.elapsed().as_millis() < ANIMATION_DURATION_MS)
            .unwrap_or(false)
    }

    pub(crate) fn has_active_agents(&self, runtime: &CoreRuntime) -> bool {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        store_ref.operations.has_active_agents()
    }

    pub(crate) fn project_display(&self, runtime: &CoreRuntime) -> String {
        match &self.current_project {
            Some(a_tag) => {
                let store = runtime.data_store();
                let store_ref = store.borrow();
                store_ref
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == *a_tag)
                    .map(|p| p.title.clone())
                    .unwrap_or_else(|| "unknown".to_string())
            }
            None => "no-project".to_string(),
        }
    }

    pub(crate) fn agent_display(&self) -> String {
        self.current_agent_name
            .clone()
            .unwrap_or_else(|| "no-agent".to_string())
    }

    pub(crate) fn switch_project(&mut self, a_tag: String, runtime: &CoreRuntime) {
        let name = {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            store_ref.get_projects().iter()
                .find(|p| p.a_tag() == a_tag)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "project".to_string())
        };
        self.current_project = Some(a_tag);
        self.start_project_animation(&name);
    }

    /// Returns (ansi_text, plain_width) for the status bar.
    pub(crate) fn status_bar_text(&self, runtime: &CoreRuntime) -> (String, usize) {
        let project = self.project_display(runtime);
        let agent = self.agent_display();

        let project_rendered = if let Some(start) = self.project_anim_start {
            let elapsed_ms = start.elapsed().as_millis() as f64;
            if elapsed_ms < ANIMATION_DURATION_F64 {
                wave_colorize(&project, elapsed_ms, &[44, 37, 73, 109, 117, 159])
            } else {
                format!("{CYAN}{project}{RESET}")
            }
        } else {
            format!("{CYAN}{project}{RESET}")
        };

        let mut text = format!("{project_rendered}{DIM}/{RESET}{GREEN}{agent}{RESET}");
        let mut plain_width = project.len() + 1 + agent.len();

        let store = runtime.data_store();
        let store_ref = store.borrow();

        if let Some(ref conv_id) = self.current_conversation {
            let working_agents = store_ref.operations.get_working_agents(conv_id);
            if !working_agents.is_empty() {
                let names: Vec<String> = working_agents
                    .iter()
                    .map(|pk| store_ref.get_profile_name(pk))
                    .collect();
                let names_str = names.join(", ");
                text.push_str(&format!(
                    "  {YELLOW}⟡ {names_str} working{RESET}"
                ));
                plain_width += 2 + 2 + names_str.len() + " working".len();
            }
        }

        for ops in store_ref.operations.get_all_active_operations() {
            let thread_id = ops.thread_id.as_deref().unwrap_or(&ops.event_id);
            if self.current_conversation.as_deref() == Some(thread_id) {
                continue;
            }
            let names: Vec<String> = ops
                .agent_pubkeys
                .iter()
                .map(|pk| store_ref.get_profile_name(pk))
                .collect();
            let title = store_ref
                .get_thread_by_id(thread_id)
                .map(|t| thread_display_name(t, 40))
                .unwrap_or_else(|| format!("{}…", &thread_id[..thread_id.len().min(12)]));
            let names_str = names.join(", ");
            text.push_str(&format!(
                "  {DIM}⚡ {names_str} → \"{title}\"{RESET}"
            ));
            plain_width += 2 + 2 + names_str.len() + " → \"".len() + title.len() + "\"".len();
        }

        (text, plain_width)
    }

    pub(crate) fn status_bar_segments(&self, runtime: &CoreRuntime) -> Vec<(String, usize)> {
        let project = self.project_display(runtime);
        let agent = self.agent_display();

        let project_rendered = if let Some(start) = self.project_anim_start {
            let elapsed_ms = start.elapsed().as_millis() as f64;
            if elapsed_ms < ANIMATION_DURATION_F64 {
                wave_colorize(&project, elapsed_ms, &[44, 37, 73, 109, 117, 159])
            } else {
                format!("{CYAN}{project}{RESET}")
            }
        } else {
            format!("{CYAN}{project}{RESET}")
        };

        let mut segments: Vec<(String, usize)> = Vec::new();
        segments.push((project_rendered, project.len()));
        segments.push((format!("{GREEN}{agent}{RESET}"), agent.len()));

        let store = runtime.data_store();
        let store_ref = store.borrow();

        for ops in store_ref.operations.get_all_active_operations() {
            let thread_id = ops.thread_id.as_deref().unwrap_or(&ops.event_id);
            if self.current_conversation.as_deref() == Some(thread_id) {
                continue;
            }
            let names: Vec<String> = ops
                .agent_pubkeys
                .iter()
                .map(|pk| store_ref.get_profile_name(pk))
                .collect();
            let title = store_ref
                .get_thread_by_id(thread_id)
                .map(|t| thread_display_name(t, 40))
                .unwrap_or_else(|| format!("{}…", &thread_id[..thread_id.len().min(12)]));
            let names_str = names.join(", ");
            let plain = format!("⚡ {} → \"{}\"", names_str, title);
            let ansi = format!("{DIM}⚡ {} → \"{title}\"{RESET}", names_str);
            segments.push((ansi, plain.len()));
        }

        segments.push(("__runtime__".to_string(), 0));
        segments
    }

    pub(crate) fn status_bar_text_navigable(&self, runtime: &CoreRuntime, focused: usize) -> (String, usize) {
        let segments = self.status_bar_segments(runtime);
        let mut text = String::new();
        let mut plain_width = 0;

        for (i, (ansi, pw)) in segments.iter().enumerate() {
            if ansi == "__runtime__" {
                continue;
            }
            if i > 0 {
                if i == 1 {
                    text.push_str(&format!("{DIM}/{RESET}"));
                    plain_width += 1;
                } else {
                    text.push_str("  ");
                    plain_width += 2;
                }
            }
            if i == focused {
                let plain_text = strip_ansi(ansi);
                text.push_str(&format!("\x1b[7m{plain_text}\x1b[27m"));
            } else {
                text.push_str(ansi);
            }
            plain_width += pw;
        }

        (text, plain_width)
    }

    pub(crate) fn status_bar_enter_action(&self, segment: usize, runtime: &CoreRuntime) -> StatusBarAction {
        let segments = self.status_bar_segments(runtime);
        if segment >= segments.len() {
            return StatusBarAction::OpenStats;
        }

        let last = segments.len().saturating_sub(1);
        if segment == last {
            return StatusBarAction::OpenStats;
        }
        if segment == 0 {
            return StatusBarAction::ShowCompletion("/project ".to_string());
        }
        if segment == 1 {
            return StatusBarAction::ShowCompletion("/agent ".to_string());
        }

        let store = runtime.data_store();
        let store_ref = store.borrow();
        let mut conv_index = 0;
        for ops in store_ref.operations.get_all_active_operations() {
            let thread_id = ops.thread_id.as_deref().unwrap_or(&ops.event_id);
            if self.current_conversation.as_deref() == Some(thread_id) {
                continue;
            }
            if conv_index + 2 == segment {
                let project_a_tag = store_ref.get_project_a_tag_for_thread(thread_id);
                return StatusBarAction::OpenConversation {
                    thread_id: thread_id.to_string(),
                    project_a_tag,
                };
            }
            conv_index += 1;
        }

        StatusBarAction::OpenStats
    }
}
