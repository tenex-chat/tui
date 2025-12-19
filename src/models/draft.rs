use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Represents a chat draft for a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDraft {
    pub conversation_id: String,
    pub text: String,
    pub selected_agent_pubkey: Option<String>,
    pub selected_branch: Option<String>,
    pub last_modified: u64,
}

impl ChatDraft {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Storage for chat drafts (persisted to JSON file)
pub struct DraftStorage {
    path: PathBuf,
    drafts: HashMap<String, ChatDraft>,
}

impl DraftStorage {
    pub fn new(data_dir: &str) -> Self {
        let path = PathBuf::from(data_dir).join("drafts.json");
        let drafts = Self::load_from_file(&path).unwrap_or_default();
        Self { path, drafts }
    }

    fn load_from_file(path: &PathBuf) -> Option<HashMap<String, ChatDraft>> {
        let contents = fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn save_to_file(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.drafts) {
            let _ = fs::write(&self.path, json);
        }
    }

    /// Save a draft for a conversation
    pub fn save(&mut self, draft: ChatDraft) {
        if draft.is_empty() {
            self.drafts.remove(&draft.conversation_id);
        } else {
            self.drafts.insert(draft.conversation_id.clone(), draft);
        }
        self.save_to_file();
    }

    /// Load a draft for a conversation
    pub fn load(&self, conversation_id: &str) -> Option<ChatDraft> {
        self.drafts.get(conversation_id).cloned()
    }

    /// Delete a draft for a conversation
    pub fn delete(&mut self, conversation_id: &str) {
        self.drafts.remove(conversation_id);
        self.save_to_file();
    }
}
