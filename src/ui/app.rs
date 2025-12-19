use crate::models::{Message, Project, Thread};
use crate::nostr::{NostrCommand, DataChange};
use crate::store::Database;
use nostr_sdk::Keys;
use std::sync::Arc;
use std::sync::mpsc::{Sender, Receiver};

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

    pub selected_project_index: usize,
    pub selected_thread_index: usize,
    pub selected_project: Option<Project>,
    pub selected_thread: Option<Thread>,

    pub scroll_offset: usize,
    pub status_message: Option<String>,

    pub creating_thread: bool,

    pub command_tx: Option<Sender<NostrCommand>>,
    pub data_rx: Option<Receiver<DataChange>>,
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

            selected_project_index: 0,
            selected_thread_index: 0,
            selected_project: None,
            selected_thread: None,

            scroll_offset: 0,
            status_message: None,

            creating_thread: false,

            command_tx: None,
            data_rx: None,
        }
    }

    pub fn set_channels(&mut self, command_tx: Sender<NostrCommand>, data_rx: Receiver<DataChange>) {
        self.command_tx = Some(command_tx);
        self.data_rx = Some(data_rx);
    }

    pub fn check_for_data_updates(&mut self) -> anyhow::Result<()> {
        if let Some(ref data_rx) = self.data_rx {
            while let Ok(change) = data_rx.try_recv() {
                match change {
                    DataChange::ProjectsUpdated => {
                        self.projects = crate::store::get_projects(&self.db.connection())?;
                    }
                    DataChange::ThreadsUpdated(project_id) => {
                        if self.selected_project.as_ref().map(|p| p.a_tag()) == Some(project_id) {
                            self.threads = crate::store::get_threads_for_project(&self.db.connection(), &self.selected_project.as_ref().unwrap().a_tag())?;
                        }
                    }
                    DataChange::MessagesUpdated(thread_id) => {
                        if self.selected_thread.as_ref().map(|t| &t.id) == Some(&thread_id) {
                            self.messages = crate::store::get_messages_for_thread(&self.db.connection(), &thread_id)?;
                        }
                    }
                    DataChange::ProfilesUpdated => {
                    }
                }
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

    pub fn submit_input(&mut self) -> String {
        let input = self.input.clone();
        self.clear_input();
        input
    }
}
