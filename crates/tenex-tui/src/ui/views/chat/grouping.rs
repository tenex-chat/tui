use crate::models::Message;

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

        // Emit delegation previews for any q_tags
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

    calculate_consecutive_states(&mut result);
    result
}
