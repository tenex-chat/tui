use crate::models::Message;

/// Known tool names that should NOT have their q-tags create QTagReference items.
/// These tools use q-tags for internal purposes (e.g., linking to reports) that
/// should not be rendered as delegation previews or inline asks.
///
/// The denylist approach ensures that ANY new tool using q-tags for delegations
/// or asks will automatically work without code changes.
const Q_TAG_RENDER_DENYLIST: &[&str] = &[
    "mcp__tenex__report_write",
    "mcp__tenex__report_read",
    "mcp__tenex__report_delete",
    "mcp__tenex__lesson_learn",
    "mcp__tenex__lesson_get",
];

/// Determines if a message's q-tags should be rendered as QTagReference items.
///
/// Returns true for ALL messages EXCEPT those from tools in the denylist.
/// This ensures that q-tag presence alone triggers rendering, and the
/// renderer decides WHAT to show based on the referenced event's type.
///
/// The denylist contains tools that use q-tags for internal linking purposes
/// (e.g., report_write links to the article it creates) rather than for
/// delegation/ask references.
pub fn should_render_q_tags(tool_name: Option<&str>) -> bool {
    match tool_name {
        // No tool name means it's a regular message - q_tags should be rendered
        None => true,
        // Check against denylist - if NOT in denylist, render q_tags
        Some(name) => !Q_TAG_RENDER_DENYLIST.contains(&name),
    }
}

/// Grouped display item - either a single message or a delegation preview
pub enum DisplayItem<'a> {
    /// Single message - every message is its own item
    SingleMessage {
        message: &'a Message,
        /// True if previous item has the same pubkey (for visual styling)
        is_consecutive: bool,
        /// True if next item has the same pubkey (for visual styling)
        has_next_consecutive: bool,
    },
    /// Delegation preview - a reference to another thread via q-tag
    DelegationPreview {
        /// The thread ID being delegated to
        thread_id: String,
        /// Pubkey of the parent message (for indicator color)
        parent_pubkey: String,
        /// True if previous item has the same pubkey
        is_consecutive: bool,
        /// True if next item has the same pubkey
        has_next_consecutive: bool,
    },
}

/// Helper to get pubkey from a DisplayItem
fn item_pubkey<'a>(item: &'a DisplayItem<'a>) -> &'a str {
    match item {
        DisplayItem::SingleMessage { message, .. } => &message.pubkey,
        DisplayItem::DelegationPreview { parent_pubkey, .. } => parent_pubkey,
    }
}

/// Calculate consecutive states by comparing pubkeys of adjacent items
fn calculate_consecutive_states(items: &mut [DisplayItem<'_>]) {
    let len = items.len();
    if len == 0 {
        return;
    }

    // First pass: collect pubkeys
    let pubkeys: Vec<String> = items.iter().map(|i| item_pubkey(i).to_string()).collect();

    // Second pass: calculate and apply states
    for i in 0..len {
        let is_consecutive = i > 0 && pubkeys[i] == pubkeys[i - 1];
        let has_next_consecutive = i < len - 1 && pubkeys[i] == pubkeys[i + 1];

        match &mut items[i] {
            DisplayItem::SingleMessage {
                is_consecutive: ic,
                has_next_consecutive: hnc,
                ..
            } => {
                *ic = is_consecutive;
                *hnc = has_next_consecutive;
            }
            DisplayItem::DelegationPreview {
                is_consecutive: ic,
                has_next_consecutive: hnc,
                ..
            } => {
                *ic = is_consecutive;
                *hnc = has_next_consecutive;
            }
        }
    }
}

/// Convert messages to display items.
/// Each message becomes its own SingleMessage item.
/// Q-tags emit QTagReference items (rendered as delegation cards or inline asks).
/// Consecutive states are calculated for visual styling only.
///
/// Q-TAG RENDERING PHILOSOPHY:
/// - Q-tag PRESENCE determines IF we render something
/// - Q-tagged event TYPE determines WHAT we render (ask UI vs delegation card)
/// - The renderer (messages.rs) checks get_ask_event_by_id() to distinguish
pub fn group_messages<'a>(messages: &[&'a Message]) -> Vec<DisplayItem<'a>> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<DisplayItem<'a>> = Vec::new();

    for msg in messages {
        // Every message is its own SingleMessage
        result.push(DisplayItem::SingleMessage {
            message: msg,
            is_consecutive: false,
            has_next_consecutive: false,
        });

        // Emit QTagReference items for q_tags
        // The denylist filters out tools that use q_tags for internal purposes
        // (e.g., report_write linking to the article it creates)
        if should_render_q_tags(msg.tool_name.as_deref()) {
            for q_tag in &msg.q_tags {
                // DelegationPreview is used for both delegations AND ask events
                // The renderer checks the event type to decide what to display
                result.push(DisplayItem::DelegationPreview {
                    thread_id: q_tag.clone(),
                    parent_pubkey: msg.pubkey.clone(),
                    is_consecutive: false,
                    has_next_consecutive: false,
                });
            }
        }
    }

    calculate_consecutive_states(&mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ==========================================================================
    // Tests for should_render_q_tags (denylist approach)
    // ==========================================================================

    #[test]
    fn test_should_render_q_tags_with_none() {
        // Regular messages (no tool) should always render q_tags
        assert!(should_render_q_tags(None));
    }

    #[test]
    fn test_should_render_q_tags_with_denylisted_tools() {
        // Tools in the denylist should NOT render q_tags
        assert!(!should_render_q_tags(Some("mcp__tenex__report_write")));
        assert!(!should_render_q_tags(Some("mcp__tenex__report_read")));
        assert!(!should_render_q_tags(Some("mcp__tenex__report_delete")));
        assert!(!should_render_q_tags(Some("mcp__tenex__lesson_learn")));
        assert!(!should_render_q_tags(Some("mcp__tenex__lesson_get")));
    }

    #[test]
    fn test_should_render_q_tags_with_delegation_tools() {
        // Delegation tools should render q_tags (not in denylist)
        assert!(should_render_q_tags(Some("mcp__tenex__delegate")));
        assert!(should_render_q_tags(Some(
            "mcp__tenex__delegate_crossproject"
        )));
        assert!(should_render_q_tags(Some("mcp__tenex__delegate_followup")));
    }

    #[test]
    fn test_should_render_q_tags_with_ask_tools() {
        // Ask tools should render q_tags (not in denylist)
        // This is the key fix - ask events now automatically work!
        assert!(should_render_q_tags(Some("mcp__tenex__ask")));
        assert!(should_render_q_tags(Some("ask")));
    }

    #[test]
    fn test_should_render_q_tags_with_unknown_tools() {
        // Unknown tools should render q_tags by default (not in denylist)
        // This ensures new tools automatically work without code changes
        assert!(should_render_q_tags(Some("some_new_tool")));
        assert!(should_render_q_tags(Some("fs_read")));
        assert!(should_render_q_tags(Some("bash")));
        assert!(should_render_q_tags(Some("edit")));
        assert!(should_render_q_tags(Some("write")));
        assert!(should_render_q_tags(Some("task")));
    }

    // ==========================================================================
    // Integration tests for group_messages
    // ==========================================================================

    /// Regression test: report_write events with q_tags must NOT produce QTagReference items.
    /// This was the original bug - report_write events were incorrectly showing as delegations.
    #[test]
    fn test_report_write_with_q_tags_does_not_produce_q_tag_reference() {
        // Create a message simulating a report_write tool call with q_tags
        let report_write_msg = Message {
            id: "test-report-write-id".to_string(),
            content: "Executing tenex's report write".to_string(),
            pubkey: "dc613b2d2e8e0e916b33f3f427cabab8ca2191cd0b3117e892bdd5a43fc899d7".to_string(),
            thread_id: "test-thread".to_string(),
            created_at: 1769348739,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            // This is the key part: report_write has q_tags that should NOT become QTagReferences
            q_tags: vec!["9e819e31ecbd1ebd3646a9f4e3a6e712fa682269f7fb08481cf602ed61445953".to_string()],
            a_tags: vec!["30023:dc613b2d2e8e0e916b33f3f427cabab8ca2191cd0b3117e892bdd5a43fc899d7:tech-debt-quality-gaps-tests-config".to_string()],
            p_tags: vec![],
            tool_name: Some("mcp__tenex__report_write".to_string()),  // report_write tool (denylisted)
            tool_args: Some(r#"{"server":"tenex","tool":"report_write"}"#.to_string()),
            llm_metadata: HashMap::new(),
            delegation_tag: None,
            branch: None,
        };

        let messages: Vec<&Message> = vec![&report_write_msg];
        let display_items = group_messages(&messages);

        // Should have exactly 1 item - the SingleMessage
        // Should NOT have a DelegationPreview despite the q_tags
        assert_eq!(
            display_items.len(),
            1,
            "Expected only 1 display item (SingleMessage), got {}",
            display_items.len()
        );

        // Verify the single item is a SingleMessage, not a DelegationPreview
        match &display_items[0] {
            DisplayItem::SingleMessage { message, .. } => {
                assert_eq!(message.id, "test-report-write-id");
                assert_eq!(
                    message.tool_name,
                    Some("mcp__tenex__report_write".to_string())
                );
            }
            DisplayItem::DelegationPreview { .. } => {
                panic!("report_write q_tags should NOT produce DelegationPreview items!");
            }
        }
    }

    /// Verify that delegation tools DO produce QTagReference items
    #[test]
    fn test_delegate_tool_with_q_tags_produces_q_tag_reference() {
        let delegate_msg = Message {
            id: "test-delegate-id".to_string(),
            content: "Delegating to agent".to_string(),
            pubkey: "dc613b2d2e8e0e916b33f3f427cabab8ca2191cd0b3117e892bdd5a43fc899d7".to_string(),
            thread_id: "test-thread".to_string(),
            created_at: 1769348739,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec!["child-conversation-id".to_string()],
            a_tags: vec![],
            p_tags: vec![],
            tool_name: Some("mcp__tenex__delegate".to_string()), // delegation tool (not in denylist)
            tool_args: None,
            llm_metadata: HashMap::new(),
            delegation_tag: None,
            branch: Some("feature-branch".to_string()),
        };

        let messages: Vec<&Message> = vec![&delegate_msg];
        let display_items = group_messages(&messages);

        // Should have 2 items: SingleMessage + DelegationPreview
        assert_eq!(
            display_items.len(),
            2,
            "Expected 2 display items, got {}",
            display_items.len()
        );

        // First should be SingleMessage
        match &display_items[0] {
            DisplayItem::SingleMessage { message, .. } => {
                assert_eq!(message.id, "test-delegate-id");
            }
            _ => panic!("First item should be SingleMessage"),
        }

        // Second should be DelegationPreview (Q-tag reference)
        match &display_items[1] {
            DisplayItem::DelegationPreview { thread_id, .. } => {
                assert_eq!(thread_id, "child-conversation-id");
            }
            _ => panic!("Second item should be DelegationPreview"),
        }
    }

    /// Critical test: ask tool with q_tags MUST produce QTagReference items
    /// This is the bug we're fixing - ask events weren't rendering because
    /// their q_tags were being filtered out.
    #[test]
    fn test_ask_tool_with_q_tags_produces_q_tag_reference() {
        let ask_msg = Message {
            id: "test-ask-id".to_string(),
            content: "Executing tenex's ask".to_string(),
            pubkey: "dc613b2d2e8e0e916b33f3f427cabab8ca2191cd0b3117e892bdd5a43fc899d7".to_string(),
            thread_id: "test-thread".to_string(),
            created_at: 1769348739,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec!["ask-event-id".to_string()],
            a_tags: vec![],
            p_tags: vec![],
            tool_name: Some("mcp__tenex__ask".to_string()), // ask tool (not in denylist)
            tool_args: None,
            llm_metadata: HashMap::new(),
            delegation_tag: None,
            branch: None,
        };

        let messages: Vec<&Message> = vec![&ask_msg];
        let display_items = group_messages(&messages);

        // Should have 2 items: SingleMessage + DelegationPreview (for the q-tag)
        assert_eq!(
            display_items.len(),
            2,
            "Ask tool q_tags MUST produce QTagReference items, got {}",
            display_items.len()
        );

        // Second should be DelegationPreview pointing to the ask event
        match &display_items[1] {
            DisplayItem::DelegationPreview { thread_id, .. } => {
                assert_eq!(thread_id, "ask-event-id");
            }
            _ => panic!("Ask tool q_tags should produce DelegationPreview items!"),
        }
    }

    /// Test the short-form "ask" tool name also works
    #[test]
    fn test_ask_short_form_tool_with_q_tags_produces_q_tag_reference() {
        let ask_msg = Message {
            id: "test-ask-short-id".to_string(),
            content: "Executing ask".to_string(),
            pubkey: "dc613b2d2e8e0e916b33f3f427cabab8ca2191cd0b3117e892bdd5a43fc899d7".to_string(),
            thread_id: "test-thread".to_string(),
            created_at: 1769348739,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec!["ask-event-id".to_string()],
            a_tags: vec![],
            p_tags: vec![],
            tool_name: Some("ask".to_string()), // short-form ask tool (not in denylist)
            tool_args: None,
            llm_metadata: HashMap::new(),
            delegation_tag: None,
            branch: None,
        };

        let messages: Vec<&Message> = vec![&ask_msg];
        let display_items = group_messages(&messages);

        // Should have 2 items: SingleMessage + DelegationPreview
        assert_eq!(
            display_items.len(),
            2,
            "Short-form 'ask' tool q_tags MUST produce QTagReference items, got {}",
            display_items.len()
        );
    }
}
