use crate::models::Message;

/// Check if a message is a delegation tool call (should never be collapsed and breaks groups)
fn is_delegation_tool(message: &Message) -> bool {
    matches!(
        message.tool_name.as_deref(),
        Some("delegate") | Some("delegate_external")
    )
}

/// Check if a message has p-tags (mentions)
fn has_p_tag(message: &Message) -> bool {
    !message.p_tags.is_empty()
}

/// Check if a message is collapsible
/// Only tool use events are collapsible (except delegations which are never collapsible)
/// Regular messages without tools are always visible
fn is_collapsible(message: &Message) -> bool {
    // Must have a tool to be collapsible
    let has_tool = message.tool_name.is_some();
    // Delegations are never collapsible
    let is_delegation = is_delegation_tool(message);

    has_tool && !is_delegation
}

/// Message visibility info for rendering within an agent group
#[derive(Debug)]
pub(crate) struct MessageVisibility<'a> {
    pub message: &'a Message,
    pub visible: bool,
}

/// Grouped display item - either a single visible message or an agent group
/// Matches Svelte's DisplayItem type
pub(crate) enum DisplayItem<'a> {
    /// Single message (from user or standalone agent message)
    SingleMessage {
        message: &'a Message,
        /// True if previous item has the same pubkey
        is_consecutive: bool,
        /// True if next item has the same pubkey
        has_next_consecutive: bool,
    },
    /// Group of consecutive agent messages (collapsed/expandable)
    AgentGroup {
        messages: Vec<&'a Message>,
        pubkey: String,
        /// True if previous item has the same pubkey
        is_consecutive: bool,
        /// True if next item has the same pubkey
        has_next_consecutive: bool,
        /// Visibility for each message in the group
        visibility: Vec<MessageVisibility<'a>>,
        /// Count of collapsed (non-visible) messages
        collapsed_count: usize,
    },
}

/// Helper to get pubkey from a DisplayItem
fn item_pubkey<'a>(item: &'a DisplayItem<'a>) -> &'a str {
    match item {
        DisplayItem::SingleMessage { message, .. } => &message.pubkey,
        DisplayItem::AgentGroup { pubkey, .. } => pubkey,
    }
}

/// Group consecutive messages from the same agent.
/// Groups break on:
/// - Different pubkey
/// - Message has p-tag (mention)
/// - Message is a delegation tool call
fn group_consecutive_agent_messages<'a>(
    messages: &[&'a Message],
    user_pubkey: Option<&str>,
) -> Vec<DisplayItem<'a>> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<DisplayItem<'a>> = Vec::new();
    let mut current_group: Vec<&'a Message> = Vec::new();
    let mut group_pubkey: Option<String> = None;

    let flush_group = |group: &mut Vec<&'a Message>,
                       pubkey: &mut Option<String>,
                       result: &mut Vec<DisplayItem<'a>>| {
        if group.is_empty() {
            return;
        }

        if group.len() == 1 {
            // Single message - emit as SingleMessage
            result.push(DisplayItem::SingleMessage {
                message: group[0],
                is_consecutive: false,
                has_next_consecutive: false,
            });
        } else {
            // Multiple messages - emit as AgentGroup with visibility calculated
            let visibility = calculate_group_visibility(group);
            let collapsed_count = visibility.iter().filter(|v| !v.visible).count();

            result.push(DisplayItem::AgentGroup {
                messages: group.clone(),
                pubkey: pubkey.clone().unwrap(),
                is_consecutive: false,
                has_next_consecutive: false,
                visibility,
                collapsed_count,
            });
        }
        group.clear();
        *pubkey = None;
    };

    for msg in messages {
        let is_user = user_pubkey.map(|pk| pk == msg.pubkey.as_str()).unwrap_or(false);
        let msg_has_p_tag = has_p_tag(msg);
        let msg_is_delegation = is_delegation_tool(msg);

        // User messages are always standalone
        if is_user {
            flush_group(&mut current_group, &mut group_pubkey, &mut result);
            result.push(DisplayItem::SingleMessage {
                message: msg,
                is_consecutive: false,
                has_next_consecutive: false,
            });
            continue;
        }

        // Check if this message should break the current group
        let should_break = group_pubkey.as_ref().map_or(false, |pk| pk != &msg.pubkey)
            || msg_has_p_tag
            || msg_is_delegation;

        if should_break {
            flush_group(&mut current_group, &mut group_pubkey, &mut result);
        }

        current_group.push(msg);
        if group_pubkey.is_none() {
            group_pubkey = Some(msg.pubkey.clone());
        }
    }

    // Flush final group
    flush_group(&mut current_group, &mut group_pubkey, &mut result);

    result
}

/// Calculate visibility for messages within an agent group
/// Rules from Svelte:
/// - If p-tag exists: collapse before p-tag, show from p-tag onwards
/// - If no p-tag (agent still working): show last 2 collapsible, collapse rest
/// - Delegations are never collapsible (always shown)
fn calculate_group_visibility<'a>(messages: &[&'a Message]) -> Vec<MessageVisibility<'a>> {
    // Find index of first p-tagged message
    let p_tag_index = messages.iter().position(|m| has_p_tag(m));

    if let Some(p_idx) = p_tag_index {
        // P-tag mode: collapse everything collapsible before p-tag
        messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let msg_collapsible = is_collapsible(msg);
                let is_before_p_tag = i < p_idx;
                // Show if: non-collapsible OR at/after p-tag
                let visible = !msg_collapsible || !is_before_p_tag;
                MessageVisibility { message: msg, visible }
            })
            .collect()
    } else {
        // No p-tag mode: show last 2 collapsible + all non-collapsible
        let collapsible_indices: Vec<usize> = messages
            .iter()
            .enumerate()
            .filter(|(_, m)| is_collapsible(m))
            .map(|(i, _)| i)
            .collect();

        // Last 2 collapsible indices
        let visible_collapsible: std::collections::HashSet<usize> = collapsible_indices
            .iter()
            .rev()
            .take(2)
            .copied()
            .collect();

        messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let msg_collapsible = is_collapsible(msg);
                // Show if: non-collapsible OR one of last 2 collapsible
                let visible = !msg_collapsible || visible_collapsible.contains(&i);
                MessageVisibility { message: msg, visible }
            })
            .collect()
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
            DisplayItem::AgentGroup {
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

/// Group consecutive messages from the same author, then calculate consecutive states.
/// Matches Svelte's createSimplifiedDisplayModel.
pub(crate) fn group_messages<'a>(
    messages: &[&'a Message],
    user_pubkey: Option<&str>,
) -> Vec<DisplayItem<'a>> {
    let mut items = group_consecutive_agent_messages(messages, user_pubkey);
    calculate_consecutive_states(&mut items);
    items
}
