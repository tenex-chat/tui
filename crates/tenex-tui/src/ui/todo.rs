use crate::models::Message;
use crate::ui::tool_calls::{parse_message_content, MessageContent};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TodoStatus,
    pub skip_reason: Option<String>,
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
        self.items
            .iter()
            .filter(|i| i.status == TodoStatus::Done)
            .count()
    }

    pub fn in_progress_item(&self) -> Option<&TodoItem> {
        self.items
            .iter()
            .find(|i| i.status == TodoStatus::InProgress)
    }
}

/// todo_write item format (backend standard)
#[derive(Debug, Deserialize)]
struct TodoWriteItem {
    id: Option<String>,
    content: Option<String>,
    title: Option<String>,
    status: Option<String>,
    #[serde(rename = "activeForm")]
    active_form: Option<String>,
    description: Option<String>,
    skip_reason: Option<String>,
}

/// MCP todo_write payload format (supports both "todos" and "items")
#[derive(Debug, Deserialize)]
struct McpTodoPayload {
    #[serde(alias = "items")]
    todos: Option<Vec<TodoWriteItem>>,
}

fn parse_status(s: &str) -> TodoStatus {
    match s.to_lowercase().as_str() {
        "done" | "completed" => TodoStatus::Done,
        "in_progress" => TodoStatus::InProgress,
        "skipped" => TodoStatus::Skipped,
        _ => TodoStatus::Pending,
    }
}

/// Check if a tool name is a todo_write variant
pub fn is_todo_write(name: &str) -> bool {
    matches!(name, "todo_write" | "todowrite" | "mcp__tenex__todo_write")
}

/// Aggregate todo state from a list of messages
/// Processes todo_write tool calls (backend standard)
/// Supports both embedded JSON tool calls (Claude Code style) and tag-based tool calls (TENEX style)
pub fn aggregate_todo_state(messages: &[Message]) -> TodoState {
    let mut items: Vec<TodoItem> = Vec::new();

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

        // Handle todo_write variants (todowrite, todo_write, mcp__tenex__todo_write)
        if is_todo_write(&tool_name) {
            // todo_write replaces the entire list
            // Support both "todos" and "items" field names
            let payload_result = serde_json::from_value::<McpTodoPayload>(parameters.clone());

            if let Ok(payload) = payload_result {
                if let Some(todos_array) = payload.todos {
                    items.clear();
                    let mut id_counter = 0usize;

                    for todo_item in todos_array {
                        let title = todo_item.content.or(todo_item.title).unwrap_or_default();

                        if !title.is_empty() {
                            // Preserve MCP-provided ID, fallback to generated ID
                            let id = todo_item.id.unwrap_or_else(|| {
                                let generated = format!("todo-{}", id_counter);
                                id_counter += 1;
                                generated
                            });

                            items.push(TodoItem {
                                id,
                                title,
                                description: todo_item.active_form.or(todo_item.description),
                                status: todo_item
                                    .status
                                    .map(|s| parse_status(&s))
                                    .unwrap_or(TodoStatus::Pending),
                                skip_reason: todo_item.skip_reason,
                            });
                        }
                    }
                }
            }
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
            a_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
            branch: None,
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
            a_tags: vec![],
            p_tags: vec![],
            tool_name: Some(tool_name.to_string()),
            tool_args: Some(tool_args.to_string()),
            llm_metadata: vec![],
            delegation_tag: None,
            branch: None,
        }
    }

    #[test]
    fn test_empty_messages() {
        let state = aggregate_todo_state(&[]);
        assert!(!state.has_todos());
    }

    #[test]
    fn test_todo_write_parsing() {
        // Test lowercase todo_write (backend standard)
        let msg = make_message(
            r#"{"name": "todo_write", "parameters": {"todos": [{"content": "First task", "status": "pending", "activeForm": "Working on first task"}, {"content": "Second task", "status": "in_progress", "activeForm": "Doing second task"}]}}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert!(state.has_todos());
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].title, "First task");
        assert_eq!(state.items[0].status, TodoStatus::Pending);
        assert_eq!(state.items[1].title, "Second task");
        assert_eq!(state.items[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn test_todo_write_pascal_case() {
        // Test TodoWrite (PascalCase) - used by different agent types
        let msg = make_message(
            r#"{"name": "TodoWrite", "parameters": {"todos": [{"content": "Task", "status": "pending"}]}}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert!(state.has_todos());
        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].title, "Task");
    }

    #[test]
    fn test_todo_write_replaces_list() {
        let msg1 = make_message(
            r#"{"name": "todo_write", "parameters": {"todos": [{"content": "Task A", "status": "pending"}]}}"#,
        );
        let msg2 = make_message(
            r#"{"name": "todo_write", "parameters": {"todos": [{"content": "Task B", "status": "done"}]}}"#,
        );

        let state = aggregate_todo_state(&[msg1, msg2]);

        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].title, "Task B");
        assert_eq!(state.items[0].status, TodoStatus::Done);
    }

    #[test]
    fn test_completed_count() {
        let msg = make_message(
            r#"{"name": "todo_write", "parameters": {"todos": [{"content": "Done task", "status": "done"}, {"content": "Pending task", "status": "pending"}, {"content": "Another done", "status": "completed"}]}}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert_eq!(state.completed_count(), 2);
    }

    #[test]
    fn test_in_progress_item() {
        let msg = make_message(
            r#"{"name": "todo_write", "parameters": {"todos": [{"content": "Done task", "status": "done"}, {"content": "Active task", "status": "in_progress"}]}}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        let in_progress = state.in_progress_item();
        assert!(in_progress.is_some());
        assert_eq!(in_progress.unwrap().title, "Active task");
    }

    #[test]
    fn test_tag_based_todo_write() {
        // TENEX-style tag-based tool call: ["tool", "todo_write"], ["tool-args", "..."]
        let msg = make_tag_based_message(
            "todo_write",
            r#"{"todos":[{"content":"First task","status":"pending"},{"content":"Second task","status":"in_progress"}]}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert!(state.has_todos());
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].title, "First task");
        assert_eq!(state.items[0].status, TodoStatus::Pending);
        assert_eq!(state.items[1].title, "Second task");
        assert_eq!(state.items[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn test_mcp_todo_write() {
        // MCP-style tag-based tool call: ["tool", "mcp__tenex__todo_write"], ["tool-args", "..."]
        let msg = make_tag_based_message(
            "mcp__tenex__todo_write",
            r#"{"todos":[{"id":"1","title":"Analyze completion event tagging","description":"Investigate why completion events (status: completed) are missing llm-runtime tags when other events in the same conversation have them","status":"in_progress"},{"id":"2","title":"Fix completion event tagging","description":"Implement fix to ensure completion events include llm-runtime tags","status":"pending"},{"id":"3","title":"Verify fix","description":"Test that completion events now properly include llm-runtime tags","status":"pending"},{"id":"4","title":"Merge to master","description":"Merge the fix to master branch and clean up worktree","status":"pending"}]}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert!(state.has_todos());
        assert_eq!(state.items.len(), 4);
        assert_eq!(state.items[0].title, "Analyze completion event tagging");
        assert_eq!(state.items[0].status, TodoStatus::InProgress);
        assert_eq!(state.items[0].description, Some("Investigate why completion events (status: completed) are missing llm-runtime tags when other events in the same conversation have them".to_string()));
        assert_eq!(state.items[1].title, "Fix completion event tagging");
        assert_eq!(state.items[1].status, TodoStatus::Pending);
        assert_eq!(state.items[2].title, "Verify fix");
        assert_eq!(state.items[2].status, TodoStatus::Pending);
        assert_eq!(state.items[3].title, "Merge to master");
        assert_eq!(state.items[3].status, TodoStatus::Pending);
    }

    #[test]
    fn test_skipped_status() {
        let msg = make_message(
            r#"{"name": "todo_write", "parameters": {"todos": [{"content": "Skipped task", "status": "skipped", "skip_reason": "No longer needed"}]}}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].status, TodoStatus::Skipped);
        assert_eq!(
            state.items[0].skip_reason,
            Some("No longer needed".to_string())
        );
    }

    #[test]
    fn test_todo_write_with_title_field() {
        // Test using title field instead of content
        let msg = make_message(
            r#"{"name": "todo_write", "parameters": {"todos": [{"title": "Task via title", "status": "pending"}]}}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].title, "Task via title");
    }

    #[test]
    fn test_mcp_todo_write_with_items_field() {
        // Test MCP payload using "items" field instead of "todos"
        let msg = make_tag_based_message(
            "mcp__tenex__todo_write",
            r#"{"items":[{"id":"item-1","title":"First item","status":"pending"},{"id":"item-2","title":"Second item","status":"done"}]}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert!(state.has_todos());
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].title, "First item");
        assert_eq!(state.items[0].status, TodoStatus::Pending);
        assert_eq!(state.items[1].title, "Second item");
        assert_eq!(state.items[1].status, TodoStatus::Done);
    }

    #[test]
    fn test_mcp_preserves_provided_ids() {
        // Test that MCP-provided IDs are preserved instead of generating new ones
        let msg = make_tag_based_message(
            "mcp__tenex__todo_write",
            r#"{"todos":[{"id":"custom-1","title":"Task A","status":"pending"},{"id":"custom-2","title":"Task B","status":"in_progress"}]}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].id, "custom-1");
        assert_eq!(state.items[0].title, "Task A");
        assert_eq!(state.items[1].id, "custom-2");
        assert_eq!(state.items[1].title, "Task B");
    }

    #[test]
    fn test_fallback_to_generated_ids() {
        // Test that generated IDs are used when MCP doesn't provide them
        let msg = make_tag_based_message(
            "todo_write",
            r#"{"todos":[{"title":"Task without ID","status":"pending"},{"title":"Another task","status":"done"}]}"#,
        );

        let state = aggregate_todo_state(&[msg]);

        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].id, "todo-0");
        assert_eq!(state.items[1].id, "todo-1");
    }

    #[test]
    fn test_is_todo_write_helper() {
        // Test the centralized helper function
        assert!(is_todo_write("todo_write"));
        assert!(is_todo_write("todowrite"));
        assert!(is_todo_write("mcp__tenex__todo_write"));
        assert!(!is_todo_write("other_tool"));
        assert!(!is_todo_write("TodoWrite")); // Case-sensitive
    }
}
