use crate::models::{Message, Project, Thread};
use crate::nostr::NostrClient;
use crate::store::Database;
use nostr_sdk::Keys;
use std::sync::Arc;

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
    pub nostr_client: Option<NostrClient>,
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
            nostr_client: None,
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
        }
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
