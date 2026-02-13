use crate::models::{AgentDefinition, Lesson, MCPTool, Nudge};
use nostrdb::{Filter, Ndb, Note, Transaction};
use std::collections::HashMap;
use std::sync::Arc;

/// Sub-store for content/definition data: agent definitions, MCP tools, nudges, lessons.
/// These are all simple keyed collections with no cross-dependencies to other domains.
pub struct ContentStore {
    pub agent_definitions: HashMap<String, AgentDefinition>,
    pub mcp_tools: HashMap<String, MCPTool>,
    pub nudges: HashMap<String, Nudge>,
    pub lessons: HashMap<String, Lesson>,
}

impl ContentStore {
    pub fn new() -> Self {
        Self {
            agent_definitions: HashMap::new(),
            mcp_tools: HashMap::new(),
            nudges: HashMap::new(),
            lessons: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.agent_definitions.clear();
        self.mcp_tools.clear();
        self.nudges.clear();
        self.lessons.clear();
    }

    // ===== Getters =====

    pub fn get_agent_definitions(&self) -> Vec<&AgentDefinition> {
        let mut defs: Vec<_> = self.agent_definitions.values().collect();
        defs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        defs
    }

    pub fn get_agent_definition(&self, id: &str) -> Option<&AgentDefinition> {
        self.agent_definitions.get(id)
    }

    pub fn get_mcp_tools(&self) -> Vec<&MCPTool> {
        let mut tools: Vec<_> = self.mcp_tools.values().collect();
        tools.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        tools
    }

    pub fn get_mcp_tool(&self, id: &str) -> Option<&MCPTool> {
        self.mcp_tools.get(id)
    }

    pub fn get_nudges(&self) -> Vec<&Nudge> {
        let mut nudges: Vec<_> = self.nudges.values().collect();
        nudges.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        nudges
    }

    pub fn get_nudge(&self, id: &str) -> Option<&Nudge> {
        self.nudges.get(id)
    }

    pub fn get_lesson(&self, lesson_id: &str) -> Option<&Lesson> {
        self.lessons.get(lesson_id)
    }

    // ===== Event Handlers =====

    pub fn handle_agent_definition_event(&mut self, note: &Note) {
        if let Some(agent_def) = AgentDefinition::from_note(note) {
            self.agent_definitions.insert(agent_def.id.clone(), agent_def);
        }
    }

    pub fn handle_mcp_tool_event(&mut self, note: &Note) {
        if let Some(tool) = MCPTool::from_note(note) {
            self.mcp_tools.insert(tool.id.clone(), tool);
        }
    }

    pub fn handle_nudge_event(&mut self, note: &Note) {
        if let Some(nudge) = Nudge::from_note(note) {
            if let Some(ref superseded_id) = nudge.supersedes {
                self.nudges.remove(superseded_id);
            }
            self.nudges.insert(nudge.id.clone(), nudge);
        }
    }

    /// Insert a lesson into the store. Returns the lesson for cross-cutting concerns
    /// (e.g., adding to agent_chatter in AppDataStore).
    pub fn insert_lesson(&mut self, note: &Note) -> Option<&Lesson> {
        let lesson = Lesson::from_note(note)?;
        let lesson_id = lesson.id.clone();
        self.lessons.insert(lesson_id.clone(), lesson);
        self.lessons.get(&lesson_id)
    }

    pub fn insert_mcp_tool(&mut self, note: &Note) {
        if let Some(tool) = MCPTool::from_note(note) {
            self.mcp_tools.insert(tool.id.clone(), tool);
        }
    }

    // ===== Loaders (rebuild from ndb) =====

    pub fn load_agent_definitions(&mut self, ndb: &Arc<Ndb>) {
        let Ok(txn) = Transaction::new(ndb) else {
            return;
        };

        let filter = Filter::new().kinds([4199]).build();
        let Ok(results) = ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        for result in results {
            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(agent_def) = AgentDefinition::from_note(&note) {
                    self.agent_definitions.insert(agent_def.id.clone(), agent_def);
                }
            }
        }
    }

    pub fn load_mcp_tools(&mut self, ndb: &Arc<Ndb>) {
        let Ok(txn) = Transaction::new(ndb) else {
            return;
        };

        let filter = Filter::new().kinds([4200]).build();
        let Ok(results) = ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        for result in results {
            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(tool) = MCPTool::from_note(&note) {
                    self.mcp_tools.insert(tool.id.clone(), tool);
                }
            }
        }
    }

    pub fn load_nudges(&mut self, ndb: &Arc<Ndb>) {
        let Ok(txn) = Transaction::new(ndb) else {
            return;
        };

        let filter = Filter::new().kinds([4201]).build();
        let Ok(results) = ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        let mut all_nudges: Vec<Nudge> = Vec::new();
        for result in results {
            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(nudge) = Nudge::from_note(&note) {
                    all_nudges.push(nudge);
                }
            }
        }

        all_nudges.sort_by_key(|n| n.created_at);

        for nudge in all_nudges {
            if let Some(ref superseded_id) = nudge.supersedes {
                self.nudges.remove(superseded_id);
            }
            self.nudges.insert(nudge.id.clone(), nudge);
        }
    }
}
