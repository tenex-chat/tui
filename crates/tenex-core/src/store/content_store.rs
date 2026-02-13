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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_agent_def(id: &str, name: &str, created_at: u64) -> AgentDefinition {
        AgentDefinition {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            d_tag: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            role: String::new(),
            instructions: String::new(),
            picture: None,
            version: None,
            model: None,
            tools: vec![],
            mcp_servers: vec![],
            use_criteria: vec![],
            created_at,
        }
    }

    fn make_test_mcp_tool(id: &str, name: &str, created_at: u64) -> MCPTool {
        MCPTool {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            d_tag: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            command: String::new(),
            parameters: None,
            capabilities: vec![],
            server_url: None,
            version: None,
            created_at,
        }
    }

    fn make_test_nudge(id: &str, title: &str, created_at: u64) -> Nudge {
        Nudge {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            title: title.to_string(),
            description: String::new(),
            content: String::new(),
            hashtags: vec![],
            created_at,
            allowed_tools: vec![],
            denied_tools: vec![],
            only_tools: vec![],
            supersedes: None,
        }
    }

    fn make_test_lesson(id: &str, title: &str, created_at: u64) -> Lesson {
        Lesson {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            title: title.to_string(),
            content: "lesson content".to_string(),
            detailed: None,
            reasoning: None,
            metacognition: None,
            reflection: None,
            category: None,
            created_at,
        }
    }

    #[test]
    fn test_empty_store_returns_empty() {
        let store = ContentStore::new();
        assert!(store.get_agent_definitions().is_empty());
        assert!(store.get_mcp_tools().is_empty());
        assert!(store.get_nudges().is_empty());
        assert!(store.get_lesson("nonexistent").is_none());
    }

    #[test]
    fn test_agent_definitions_sorted_descending() {
        let mut store = ContentStore::new();
        store.agent_definitions.insert("a1".to_string(), make_test_agent_def("a1", "Older Agent", 100));
        store.agent_definitions.insert("a2".to_string(), make_test_agent_def("a2", "Newer Agent", 200));
        store.agent_definitions.insert("a3".to_string(), make_test_agent_def("a3", "Middle Agent", 150));

        let defs = store.get_agent_definitions();
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[0].name, "Newer Agent");
        assert_eq!(defs[1].name, "Middle Agent");
        assert_eq!(defs[2].name, "Older Agent");
    }

    #[test]
    fn test_agent_definition_lookup() {
        let mut store = ContentStore::new();
        store.agent_definitions.insert("a1".to_string(), make_test_agent_def("a1", "Agent One", 100));

        assert!(store.get_agent_definition("a1").is_some());
        assert_eq!(store.get_agent_definition("a1").unwrap().name, "Agent One");
        assert!(store.get_agent_definition("nonexistent").is_none());
    }

    #[test]
    fn test_mcp_tools_sorted_descending() {
        let mut store = ContentStore::new();
        store.mcp_tools.insert("t1".to_string(), make_test_mcp_tool("t1", "Old Tool", 100));
        store.mcp_tools.insert("t2".to_string(), make_test_mcp_tool("t2", "New Tool", 200));

        let tools = store.get_mcp_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "New Tool");
        assert_eq!(tools[1].name, "Old Tool");
    }

    #[test]
    fn test_mcp_tool_lookup() {
        let mut store = ContentStore::new();
        store.mcp_tools.insert("t1".to_string(), make_test_mcp_tool("t1", "Tool One", 100));

        assert!(store.get_mcp_tool("t1").is_some());
        assert!(store.get_mcp_tool("missing").is_none());
    }

    #[test]
    fn test_nudges_sorted_descending() {
        let mut store = ContentStore::new();
        store.nudges.insert("n1".to_string(), make_test_nudge("n1", "Old Nudge", 100));
        store.nudges.insert("n2".to_string(), make_test_nudge("n2", "New Nudge", 200));

        let nudges = store.get_nudges();
        assert_eq!(nudges.len(), 2);
        assert_eq!(nudges[0].title, "New Nudge");
        assert_eq!(nudges[1].title, "Old Nudge");
    }

    #[test]
    fn test_lesson_lookup() {
        let mut store = ContentStore::new();
        store.lessons.insert("l1".to_string(), make_test_lesson("l1", "Lesson One", 100));

        assert!(store.get_lesson("l1").is_some());
        assert_eq!(store.get_lesson("l1").unwrap().title, "Lesson One");
        assert!(store.get_lesson("missing").is_none());
    }

    #[test]
    fn test_cleared_on_clear() {
        let mut store = ContentStore::new();
        store.agent_definitions.insert("a1".to_string(), make_test_agent_def("a1", "Agent", 100));
        store.mcp_tools.insert("t1".to_string(), make_test_mcp_tool("t1", "Tool", 100));
        store.nudges.insert("n1".to_string(), make_test_nudge("n1", "Nudge", 100));
        store.lessons.insert("l1".to_string(), make_test_lesson("l1", "Lesson", 100));

        store.clear();

        assert!(store.get_agent_definitions().is_empty());
        assert!(store.get_mcp_tools().is_empty());
        assert!(store.get_nudges().is_empty());
        assert!(store.get_lesson("l1").is_none());
    }
}
