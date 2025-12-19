use crate::models::{Agent, Message, Project, ProjectStatus, StreamingAccumulator, Thread};
use crate::nostr::{DataChange, NostrCommand};
use crate::store::Database;
use nostr_sdk::Keys;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use tracing::{debug, info_span};

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Login,
    Projects,
    Threads,
    Chat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

pub struct App {
    pub running: bool,
    pub view: View,
    pub input_mode: InputMode,
    pub input: String,
    pub cursor_position: usize,

    pub db: Arc<Database>,
    pub keys: Option<Keys>,

    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
    pub messages: Vec<Message>,
    pub agents: Vec<Agent>,

    pub selected_project_index: usize,
    pub selected_thread_index: usize,
    pub selected_project: Option<Project>,
    pub selected_thread: Option<Thread>,
    pub selected_agent: Option<Agent>,
    pub project_status: Option<ProjectStatus>,

    pub scroll_offset: usize,
    pub status_message: Option<String>,

    pub creating_thread: bool,
    pub showing_agent_selector: bool,
    pub agent_selector_index: usize,

    pub streaming_accumulator: StreamingAccumulator,

    pub command_tx: Option<Sender<NostrCommand>>,
    pub data_rx: Option<Receiver<DataChange>>,

    /// Cached profile names to avoid repeated DB lookups during render
    profile_name_cache: RefCell<HashMap<String, String>>,

    /// Filter text for projects view (type to filter)
    pub project_filter: String,

    /// Cached project status for each project (keyed by a_tag)
    project_status_cache: RefCell<HashMap<String, Option<ProjectStatus>>>,

    /// Whether offline projects section is expanded
    pub offline_projects_expanded: bool,
}

impl App {
    pub fn new(db: Database) -> Self {
        Self {
            running: true,
            view: View::Login,
            input_mode: InputMode::Normal,
            input: String::new(),
            cursor_position: 0,

            db: Arc::new(db),
            keys: None,

            projects: Vec::new(),
            threads: Vec::new(),
            messages: Vec::new(),
            agents: Vec::new(),

            selected_project_index: 0,
            selected_thread_index: 0,
            selected_project: None,
            selected_thread: None,
            selected_agent: None,
            project_status: None,

            scroll_offset: 0,
            status_message: None,

            creating_thread: false,
            showing_agent_selector: false,
            agent_selector_index: 0,

            streaming_accumulator: StreamingAccumulator::new(),

            command_tx: None,
            data_rx: None,

            profile_name_cache: RefCell::new(HashMap::new()),
            project_filter: String::new(),
            project_status_cache: RefCell::new(HashMap::new()),
            offline_projects_expanded: false,
        }
    }

    /// Get a profile name, using cache to avoid repeated DB lookups
    pub fn get_profile_name(&self, pubkey: &str) -> String {
        // Check cache first
        if let Some(name) = self.profile_name_cache.borrow().get(pubkey) {
            return name.clone();
        }

        // Lookup and cache
        let name = crate::store::get_profile_name(&self.db.ndb, pubkey);
        self.profile_name_cache.borrow_mut().insert(pubkey.to_string(), name.clone());
        name
    }

    /// Clear the profile cache (call when profiles are updated)
    pub fn clear_profile_cache(&self) {
        self.profile_name_cache.borrow_mut().clear();
    }

    /// Get project status for a project, using cache
    pub fn get_project_status_cached(&self, project: &Project) -> Option<ProjectStatus> {
        let a_tag = project.a_tag();

        // Check cache first
        if let Some(status) = self.project_status_cache.borrow().get(&a_tag) {
            return status.clone();
        }

        // Lookup and cache
        let status = crate::store::get_project_status(&self.db.ndb, &a_tag);
        self.project_status_cache
            .borrow_mut()
            .insert(a_tag, status.clone());
        status
    }

    /// Clear the project status cache (call when project status updates)
    pub fn clear_project_status_cache(&self) {
        self.project_status_cache.borrow_mut().clear();
    }

    /// Check if a project is online (has recent 24010 status)
    pub fn is_project_online(&self, project: &Project) -> bool {
        self.get_project_status_cached(project)
            .map(|s| s.is_online())
            .unwrap_or(false)
    }

    /// Get filtered projects based on current filter
    pub fn filtered_projects(&self) -> (Vec<&Project>, Vec<&Project>) {
        let filter = self.project_filter.to_lowercase();
        let matching: Vec<&Project> = self
            .projects
            .iter()
            .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
            .collect();

        // Separate into online and offline
        let (online, offline): (Vec<_>, Vec<_>) = matching
            .into_iter()
            .partition(|p| self.is_project_online(p));

        (online, offline)
    }

    pub fn set_channels(&mut self, command_tx: Sender<NostrCommand>, data_rx: Receiver<DataChange>) {
        self.command_tx = Some(command_tx);
        self.data_rx = Some(data_rx);
    }

    pub fn check_for_data_updates(&mut self) -> anyhow::Result<()> {
        if let Some(ref data_rx) = self.data_rx {
            // Limit events processed per frame to prevent UI blocking
            let mut events_processed = 0;
            const MAX_EVENTS_PER_FRAME: usize = 10;

            // Track what needs refreshing to avoid duplicate queries
            let mut needs_projects_refresh = false;
            let mut needs_threads_refresh = false;
            let mut needs_messages_refresh = false;
            let mut needs_agents_refresh = false;

            while events_processed < MAX_EVENTS_PER_FRAME {
                let change = match data_rx.try_recv() {
                    Ok(c) => c,
                    Err(_) => break,
                };
                events_processed += 1;

                debug!("Processing data change: {:?}", change);

                match change {
                    DataChange::ProjectsUpdated => {
                        needs_projects_refresh = true;
                    }
                    DataChange::ThreadsUpdated(project_id) => {
                        if self.selected_project.as_ref().map(|p| p.a_tag()) == Some(project_id) {
                            needs_threads_refresh = true;
                        }
                    }
                    DataChange::MessagesUpdated(thread_id) => {
                        if self.selected_thread.as_ref().map(|t| &t.id) == Some(&thread_id) {
                            needs_messages_refresh = true;
                            // Clear streaming content for this thread since we have the final message
                            self.streaming_accumulator.clear_message(&thread_id);
                        }
                    }
                    DataChange::ProfilesUpdated => {
                        self.clear_profile_cache();
                    }
                    DataChange::AgentsUpdated => {
                        needs_agents_refresh = true;
                    }
                    DataChange::ProjectStatusUpdated(project_coord) => {
                        // Clear cache so online/offline status updates
                        self.clear_project_status_cache();

                        if self.selected_project.as_ref().map(|p| p.a_tag()) == Some(project_coord.clone()) {
                            let _span = info_span!("get_project_status").entered();
                            self.project_status = crate::store::get_project_status(&self.db.ndb, &project_coord);
                            // Auto-select PM agent when status arrives
                            if self.selected_agent.is_none() {
                                if let Some(ref status) = self.project_status {
                                    if let Some(pm) = status.pm_agent() {
                                        self.selected_agent = crate::store::get_agent_by_pubkey(&self.db.ndb, &pm.pubkey);
                                    }
                                }
                            }
                        }
                    }
                    DataChange::StreamingDelta { message_id, delta } => {
                        let streaming_delta = crate::models::StreamingDelta {
                            message_id: message_id.clone(),
                            delta,
                            sequence: None,
                            created_at: 0,
                        };
                        self.streaming_accumulator.add_delta(streaming_delta);
                    }
                    DataChange::ConversationMetadataUpdated => {
                        // Mark threads for refresh (titles may have changed)
                        if self.selected_project.is_some() {
                            needs_threads_refresh = true;
                        }
                    }
                }
            }

            // Now do the actual refreshes (deduplicated)
            if needs_projects_refresh {
                let _span = info_span!("refresh_projects").entered();
                self.projects = crate::store::get_projects(&self.db.ndb)?;
            }
            if needs_threads_refresh {
                if let Some(ref project) = self.selected_project {
                    let _span = info_span!("refresh_threads").entered();
                    self.threads = crate::store::get_threads_for_project(&self.db.ndb, &project.a_tag())?;
                }
            }
            if needs_messages_refresh {
                if let Some(ref thread) = self.selected_thread {
                    let _span = info_span!("refresh_messages").entered();
                    self.messages = crate::store::get_messages_for_thread(&self.db.ndb, &thread.id)?;
                    self.scroll_offset = usize::MAX;
                }
            }
            if needs_agents_refresh {
                let _span = info_span!("refresh_agents").entered();
                self.agents = crate::store::get_agents(&self.db.ndb)?;
                if self.selected_agent.is_none() {
                    if let Some(ref status) = self.project_status {
                        if let Some(pm) = status.pm_agent() {
                            self.selected_agent = crate::store::get_agent_by_pubkey(&self.db.ndb, &pm.pubkey);
                        }
                    }
                }
            }

            if events_processed > 0 {
                debug!("Processed {} data events this frame", events_processed);
            }
        }
        Ok(())
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 && !self.input.is_empty() {
            self.cursor_position -= 1;
            self.input.remove(self.cursor_position);
        }
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
    }

    /// Get available agents from project status
    pub fn available_agents(&self) -> Vec<&crate::models::ProjectAgent> {
        self.project_status
            .as_ref()
            .map(|s| s.agents.iter().collect())
            .unwrap_or_default()
    }

    /// Select agent by index from available agents
    pub fn select_agent_by_index(&mut self, index: usize) {
        if let Some(ref status) = self.project_status {
            if let Some(agent) = status.agents.get(index) {
                self.selected_agent = crate::store::get_agent_by_pubkey(&self.db.ndb, &agent.pubkey);
            }
        }
    }

    pub fn submit_input(&mut self) -> String {
        let input = self.input.clone();
        self.clear_input();
        input
    }
}
