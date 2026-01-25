use crate::models::Message;

/// Known delegation tool names that should have their q-tags treated as delegations.
/// This explicit allowlist prevents false positives from substring matching.
const DELEGATION_TOOLS: &[&str] = &[
    "mcp__tenex__delegate",
    "mcp__tenex__delegate_crossproject",
    "mcp__tenex__delegate_followup",
];

/// Determines if a message's q-tags should be treated as delegation references.
///
/// Returns true if:
/// - The tool name is None (regular message, not a tool call - q_tags are valid delegations)
/// - The tool name is in the explicit DELEGATION_TOOLS allowlist
///
/// This prevents non-delegation tools (like report_write) from having their
/// q-tags mistakenly treated as delegations, which caused them to appear in
/// the DELEGATIONS sidebar section.
pub fn should_treat_as_delegation(tool_name: Option<&str>) -> bool {
    match tool_name {
        // No tool name means it's a regular message - q_tags are valid delegations
        None => true,
        // Check against explicit allowlist of delegation tools
        Some(name) => DELEGATION_TOOLS.contains(&name),
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
        /// Branch name from the delegation event (if any)
        branch: Option<String>,
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
    let pubkeys: Vec<String> = items
        .iter()
        .map(|i| item_pubkey(i).to_string())
        .collect();

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
/// Q-tags emit DelegationPreview items after their parent message.
/// Consecutive states are calculated for visual styling only.
pub fn group_messages<'a>(
    messages: &[&'a Message],
) -> Vec<DisplayItem<'a>> {
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

        // Emit delegation previews for q_tags, but only if this message should be treated as a delegation
        // Non-delegation tools (like report_write) may have q_tags for other purposes
        if should_treat_as_delegation(msg.tool_name.as_deref()) {
            for q_tag in &msg.q_tags {
                result.push(DisplayItem::DelegationPreview {
                    thread_id: q_tag.clone(),
                    parent_pubkey: msg.pubkey.clone(),
                    is_consecutive: false,
                    has_next_consecutive: false,
                    branch: msg.branch.clone(),
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

    #[test]
    fn test_should_treat_as_delegation_with_none() {
        // Regular messages (no tool) should treat q_tags as delegations
        assert!(should_treat_as_delegation(None));
    }

    #[test]
    fn test_should_treat_as_delegation_with_allowlisted_tools() {
        // Only explicit allowlisted delegation tools should be recognized
        assert!(should_treat_as_delegation(Some("mcp__tenex__delegate")));
        assert!(should_treat_as_delegation(Some("mcp__tenex__delegate_crossproject")));
        assert!(should_treat_as_delegation(Some("mcp__tenex__delegate_followup")));
    }

    #[test]
    fn test_should_treat_as_delegation_with_non_delegation_tools() {
        // Non-delegation tools should NOT treat q_tags as delegations
        assert!(!should_treat_as_delegation(Some("mcp__tenex__report_write")));
        assert!(!should_treat_as_delegation(Some("mcp__tenex__ask")));
        assert!(!should_treat_as_delegation(Some("mcp__tenex__lesson_learn")));
        assert!(!should_treat_as_delegation(Some("fs_read")));
        assert!(!should_treat_as_delegation(Some("bash")));
        assert!(!should_treat_as_delegation(Some("edit")));
        assert!(!should_treat_as_delegation(Some("write")));
        assert!(!should_treat_as_delegation(Some("task")));
    }

    #[test]
    fn test_should_treat_as_delegation_rejects_similar_names() {
        // Verify allowlist is exact-match, not substring-based
        // These should NOT match even though they contain "delegate"
        assert!(!should_treat_as_delegation(Some("delegate")));  // Not in allowlist
        assert!(!should_treat_as_delegation(Some("Delegate")));  // Case mismatch
        assert!(!should_treat_as_delegation(Some("DELEGATE")));  // Case mismatch
        assert!(!should_treat_as_delegation(Some("delegate_followup")));  // Missing prefix
        assert!(!should_treat_as_delegation(Some("some_delegate_tool")));  // Wrong format
    }

    /// Regression test: report_write events with q_tags must NOT produce DelegationPreview items.
    /// This was the original bug - report_write events were incorrectly showing as delegations.
    #[test]
    fn test_report_write_with_q_tags_does_not_produce_delegation_preview() {
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
            // This is the key part: report_write has q_tags that should NOT become DelegationPreviews
            q_tags: vec!["9e819e31ecbd1ebd3646a9f4e3a6e712fa682269f7fb08481cf602ed61445953".to_string()],
            a_tags: vec!["30023:dc613b2d2e8e0e916b33f3f427cabab8ca2191cd0b3117e892bdd5a43fc899d7:tech-debt-quality-gaps-tests-config".to_string()],
            p_tags: vec![],
            tool_name: Some("mcp__tenex__report_write".to_string()),  // report_write tool
            tool_args: Some(r#"{"server":"tenex","tool":"report_write"}"#.to_string()),
            llm_metadata: vec![],
            delegation_tag: None,
            branch: None,
        };

        let messages: Vec<&Message> = vec![&report_write_msg];
        let display_items = group_messages(&messages);

        // Should have exactly 1 item - the SingleMessage
        // Should NOT have a DelegationPreview despite the q_tags
        assert_eq!(display_items.len(), 1, "Expected only 1 display item (SingleMessage), got {}", display_items.len());

        // Verify the single item is a SingleMessage, not a DelegationPreview
        match &display_items[0] {
            DisplayItem::SingleMessage { message, .. } => {
                assert_eq!(message.id, "test-report-write-id");
                assert_eq!(message.tool_name, Some("mcp__tenex__report_write".to_string()));
            }
            DisplayItem::DelegationPreview { .. } => {
                panic!("report_write q_tags should NOT produce DelegationPreview items!");
            }
        }
    }

    /// Verify that actual delegation tools DO produce DelegationPreview items
    #[test]
    fn test_delegate_tool_with_q_tags_produces_delegation_preview() {
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
            tool_name: Some("mcp__tenex__delegate".to_string()),  // delegation tool
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
            branch: Some("feature-branch".to_string()),
        };

        let messages: Vec<&Message> = vec![&delegate_msg];
        let display_items = group_messages(&messages);

        // Should have 2 items: SingleMessage + DelegationPreview
        assert_eq!(display_items.len(), 2, "Expected 2 display items, got {}", display_items.len());

        // First should be SingleMessage
        match &display_items[0] {
            DisplayItem::SingleMessage { message, .. } => {
                assert_eq!(message.id, "test-delegate-id");
            }
            _ => panic!("First item should be SingleMessage"),
        }

        // Second should be DelegationPreview
        match &display_items[1] {
            DisplayItem::DelegationPreview { thread_id, branch, .. } => {
                assert_eq!(thread_id, "child-conversation-id");
                assert_eq!(branch, &Some("feature-branch".to_string()));
            }
            _ => panic!("Second item should be DelegationPreview"),
        }
    }
}
