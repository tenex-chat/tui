use crate::models::Message;
use crate::ui::tool_calls::{parse_message_content, MessageContent};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TodoStatus,
}

#[derive(Debug, Clone)]
pub struct TodoState {
    pub items: Vec<TodoItem>,
}

impl TodoState {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn has_todos(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn completed_count(&self) -> usize {
        self.items.iter().filter(|i| i.status == TodoStatus::Done).count()
    }

    pub fn in_progress_item(&self) -> Option<&TodoItem> {
        self.items.iter().find(|i| i.status == TodoStatus::InProgress)
    }
}

/// Claude Code's TodoWrite format
#[derive(Debug, Deserialize)]
struct TodoWriteItem {
    content: Option<String>,
    title: Option<String>,
    status: Option<String>,
    #[serde(rename = "activeForm")]
    active_form: Option<String>,
    description: Option<String>,
}

/// todo_add/todo_update format
#[derive(Debug, Deserialize)]
struct TodoAddItem {
    id: Option<String>,
    title: Option<String>,
    content: Option<String>,
    description: Option<String>,
    status: Option<String>,
}

fn parse_status(s: &str) -> TodoStatus {
    match s.to_lowercase().as_str() {
        "done" | "completed" => TodoStatus::Done,
        "in_progress" => TodoStatus::InProgress,
        _ => TodoStatus::Pending,
    }
}

/// Aggregate todo state from a list of messages
/// Processes TodoWrite, todo_add, and todo_update tool calls
/// Supports both embedded JSON tool calls (Claude Code style) and tag-based tool calls (TENEX style)
pub fn aggregate_todo_state(messages: &[Message]) -> TodoState {
    let mut items: Vec<TodoItem> = Vec::new();
    let mut id_counter = 0usize;

    for msg in messages {
        // First try content-based parsing (embedded JSON)
        let parsed = parse_message_content(&msg.content);

        let (tool_name, parameters) = match parsed {
            MessageContent::Mixed { tool_calls, .. } if !tool_calls.is_empty() => {
                // Use the first tool call from content
                let tc = &tool_calls[0];
                (tc.name.to_lowercase(), tc.parameters.clone())
            }
            _ => {
                // Fallback to tag-based tool calls (TENEX style)
                // Tags: ["tool", "tool_name"], ["tool-args", "json_string"]
                if let (Some(name), Some(args)) = (&msg.tool_name, &msg.tool_args) {
                    match serde_json::from_str::<serde_json::Value>(args) {
                        Ok(params) => (name.to_lowercase(), params),
                        Err(_) => continue,
                    }
                } else {
                    continue;
                }
            }
        };

        match tool_name.as_str() {
            "todowrite" => {
                // TodoWrite replaces the entire list
                if let Some(todos) = parameters.get("todos") {
                    if let Some(todos_array) = todos.as_array() {
                        items.clear();
                        id_counter = 0;

                        for item in todos_array {
                            if let Ok(todo_item) = serde_json::from_value::<TodoWriteItem>(item.clone()) {
                                let title = todo_item.content
                                    .or(todo_item.title)
                                    .unwrap_or_default();

                                if !title.is_empty() {
                                    let id = format!("todo-{}", id_counter);
                                    id_counter += 1;

                                    items.push(TodoItem {
                                        id,
                                        title,
                                        description: todo_item.active_form.or(todo_item.description),
                                        status: todo_item.status
                                            .map(|s| parse_status(&s))
                                            .unwrap_or(TodoStatus::Pending),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            "todo_add" => {
                if let Some(items_val) = parameters.get("items") {
                    if let Some(items_array) = items_val.as_array() {
                        for item in items_array {
                            if let Ok(todo_item) = serde_json::from_value::<TodoAddItem>(item.clone()) {
                                let title = todo_item.title
                                    .or(todo_item.content)
                                    .unwrap_or_default();

                                if !title.is_empty() {
                                    let id = todo_item.id
                                        .unwrap_or_else(|| {
                                            let id = format!("todo-{}", id_counter);
                                            id_counter += 1;
                                            id
                                        });

                                    items.push(TodoItem {
                                        id,
                                        title,
                                        description: todo_item.description,
                                        status: todo_item.status
                                            .map(|s| parse_status(&s))
                                            .unwrap_or(TodoStatus::Pending),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            "todo_update" => {
                if let Some(items_val) = parameters.get("items") {
                    if let Some(items_array) = items_val.as_array() {
                        for item in items_array {
                            if let Ok(update) = serde_json::from_value::<TodoAddItem>(item.clone()) {
                                if let Some(ref update_id) = update.id {
                                    // Find and update existing item
                                    if let Some(existing) = items.iter_mut().find(|i| &i.id == update_id) {
                                        if let Some(title) = update.title.or(update.content) {
                                            existing.title = title;
                                        }
                                        if let Some(desc) = update.description {
                                            existing.description = Some(desc);
                                        }
                                        if let Some(status) = update.status {
                                            existing.status = parse_status(&status);
                                        }
                                    } else {
                                        // Create if doesn't exist
                                        let title = update.title
                                            .or(update.content)
                                            .unwrap_or_else(|| update_id.clone());

                                        items.push(TodoItem {
                                            id: update_id.clone(),
                                            title,
                                            description: update.description,
                                            status: update.status
                                                .map(|s| parse_status(&s))
                                                .unwrap_or(TodoStatus::Pending),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    TodoState { items }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(content: &str) -> Message {
        Message {
            id: "test".to_string(),
            content: content.to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
        }
    }

    fn make_tag_based_message(tool_name: &str, tool_args: &str) -> Message {
        Message {
            id: "test".to_string(),
            content: "Tool call message".to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: Some(tool_name.to_string()),
            tool_args: Some(tool_args.to_string()),
            llm_metadata: vec![],
            delegation_tag: None,
        }
    }

    #[test]
    fn test_empty_messages() {
        let state = aggregate_todo_state(&[]);
        assert!(!state.has_todos());
    }

    #[test]
    fn test_todowrite_parsing() {
        let msg = make_message(r#"{"name": "TodoWrite", "parameters": {"todos": [{"content": "First task", "status": "pending", "activeForm": "Working on first task"}, {"content": "Second task", "status": "in_progress", "activeForm": "Doing second task"}]}}"#);

        let state = aggregate_todo_state(&[msg]);

        assert!(state.has_todos());
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].title, "First task");
        assert_eq!(state.items[0].status, TodoStatus::Pending);
        assert_eq!(state.items[1].title, "Second task");
        assert_eq!(state.items[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn test_todowrite_replaces_list() {
        let msg1 = make_message(r#"{"name": "TodoWrite", "parameters": {"todos": [{"content": "Task A", "status": "pending"}]}}"#);
        let msg2 = make_message(r#"{"name": "TodoWrite", "parameters": {"todos": [{"content": "Task B", "status": "done"}]}}"#);

        let state = aggregate_todo_state(&[msg1, msg2]);

        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].title, "Task B");
        assert_eq!(state.items[0].status, TodoStatus::Done);
    }

    #[test]
    fn test_completed_count() {
        let msg = make_message(r#"{"name": "TodoWrite", "parameters": {"todos": [{"content": "Done task", "status": "done"}, {"content": "Pending task", "status": "pending"}, {"content": "Another done", "status": "completed"}]}}"#);

        let state = aggregate_todo_state(&[msg]);

        assert_eq!(state.completed_count(), 2);
    }

    #[test]
    fn test_in_progress_item() {
        let msg = make_message(r#"{"name": "TodoWrite", "parameters": {"todos": [{"content": "Done task", "status": "done"}, {"content": "Active task", "status": "in_progress"}]}}"#);

        let state = aggregate_todo_state(&[msg]);

        let in_progress = state.in_progress_item();
        assert!(in_progress.is_some());
        assert_eq!(in_progress.unwrap().title, "Active task");
    }

    #[test]
    fn test_tag_based_todo_add() {
        // TENEX-style tag-based tool call: ["tool", "todo_add"], ["tool-args", "..."]
        let msg = make_tag_based_message(
            "todo_add",
            r#"{"items":[{"id":"task-1","title":"First task","status":"pending"},{"id":"task-2","title":"Second task","status":"in_progress"}]}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert!(state.has_todos());
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].id, "task-1");
        assert_eq!(state.items[0].title, "First task");
        assert_eq!(state.items[0].status, TodoStatus::Pending);
        assert_eq!(state.items[1].id, "task-2");
        assert_eq!(state.items[1].title, "Second task");
        assert_eq!(state.items[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn test_tag_based_todo_update() {
        // First add items via tag-based todo_add
        let msg1 = make_tag_based_message(
            "todo_add",
            r#"{"items":[{"id":"task-1","title":"Original title","status":"pending"}]}"#,
        );
        // Then update via tag-based todo_update
        let msg2 = make_tag_based_message(
            "todo_update",
            r#"{"items":[{"id":"task-1","title":"Updated title","status":"done"}]}"#,
        );

        let state = aggregate_todo_state(&[msg1, msg2]);

        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].title, "Updated title");
        assert_eq!(state.items[0].status, TodoStatus::Done);
    }
}
