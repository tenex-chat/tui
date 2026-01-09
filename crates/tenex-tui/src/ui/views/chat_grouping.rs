use crate::models::Message;

/// Grouped display item - either a single message or a collapsed group
/// Includes consecutive state for visual rendering (like Svelte's message grouping)
pub(crate) enum DisplayItem<'a> {
    SingleMessage {
        message: &'a Message,
        /// True if previous message has the same pubkey (show dot instead of header)
        is_consecutive: bool,
        /// True if next message has the same pubkey (extend vertical line)
        has_next_consecutive: bool,
    },
    ActionGroup {
        messages: Vec<&'a Message>,
        pubkey: String,
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
        DisplayItem::ActionGroup { pubkey, .. } => pubkey,
    }
}

/// Group consecutive action messages from the same author, then calculate consecutive states
pub(crate) fn group_messages<'a>(
    messages: &[&'a Message],
    user_pubkey: Option<&str>,
) -> Vec<DisplayItem<'a>> {
    // Phase 1: Group action messages (without consecutive state - set to false temporarily)
    let mut intermediate = Vec::new();
    let mut current_group: Vec<&'a Message> = Vec::new();
    let mut group_pubkey: Option<String> = None;

    for msg in messages {
        let is_user = user_pubkey.map(|pk| pk == msg.pubkey.as_str()).unwrap_or(false);
        let is_action = !is_user && is_action_message(&msg.content);

        if is_action {
            // Check if we can add to current group
            if let Some(ref pk) = group_pubkey {
                if pk == &msg.pubkey {
                    current_group.push(msg);
                    continue;
                }
            }

            // Flush existing group if any
            if !current_group.is_empty() {
                if current_group.len() == 1 {
                    intermediate.push(DisplayItem::SingleMessage {
                        message: current_group[0],
                        is_consecutive: false,
                        has_next_consecutive: false,
                    });
                } else {
                    intermediate.push(DisplayItem::ActionGroup {
                        messages: current_group.clone(),
                        pubkey: group_pubkey.clone().unwrap(),
                        is_consecutive: false,
                        has_next_consecutive: false,
                    });
                }
                current_group.clear();
            }

            // Start new group
            group_pubkey = Some(msg.pubkey.clone());
            current_group.push(msg);
        } else {
            // Flush any existing group
            if !current_group.is_empty() {
                if current_group.len() == 1 {
                    intermediate.push(DisplayItem::SingleMessage {
                        message: current_group[0],
                        is_consecutive: false,
                        has_next_consecutive: false,
                    });
                } else {
                    intermediate.push(DisplayItem::ActionGroup {
                        messages: current_group.clone(),
                        pubkey: group_pubkey.clone().unwrap(),
                        is_consecutive: false,
                        has_next_consecutive: false,
                    });
                }
                current_group.clear();
                group_pubkey = None;
            }

            intermediate.push(DisplayItem::SingleMessage {
                message: msg,
                is_consecutive: false,
                has_next_consecutive: false,
            });
        }
    }

    // Flush final group
    if !current_group.is_empty() {
        if current_group.len() == 1 {
            intermediate.push(DisplayItem::SingleMessage {
                message: current_group[0],
                is_consecutive: false,
                has_next_consecutive: false,
            });
        } else {
            intermediate.push(DisplayItem::ActionGroup {
                messages: current_group,
                pubkey: group_pubkey.unwrap(),
                is_consecutive: false,
                has_next_consecutive: false,
            });
        }
    }

    // Phase 2: Calculate consecutive states by comparing pubkeys of adjacent items
    let len = intermediate.len();
    let mut result = Vec::with_capacity(len);

    for (i, item) in intermediate.into_iter().enumerate() {
        let prev_pubkey = if i > 0 { Some(item_pubkey(&result[i - 1])) } else { None };
        let current_pubkey = item_pubkey(&item);
        let is_consecutive = prev_pubkey.map(|pk| pk == current_pubkey).unwrap_or(false);

        // We'll set has_next_consecutive in a second pass or update previous item
        match item {
            DisplayItem::SingleMessage { message, .. } => {
                result.push(DisplayItem::SingleMessage {
                    message,
                    is_consecutive,
                    has_next_consecutive: false, // Will update in next iteration
                });
            }
            DisplayItem::ActionGroup { messages, pubkey, .. } => {
                result.push(DisplayItem::ActionGroup {
                    messages,
                    pubkey,
                    is_consecutive,
                    has_next_consecutive: false,
                });
            }
        }

        // Update previous item's has_next_consecutive if current is consecutive
        if is_consecutive && i > 0 {
            let prev_idx = i - 1;
            match &mut result[prev_idx] {
                DisplayItem::SingleMessage { has_next_consecutive, .. } => {
                    *has_next_consecutive = true;
                }
                DisplayItem::ActionGroup { has_next_consecutive, .. } => {
                    *has_next_consecutive = true;
                }
            }
        }
    }

    result
}

/// Check if a message looks like a short action/status message
fn is_action_message(content: &str) -> bool {
    let trimmed = content.trim();

    // Must be short (under 100 chars) and single line
    if trimmed.len() > 100 || trimmed.contains('\n') {
        return false;
    }

    // Common action patterns
    let action_patterns = [
        "Sending", "Executing", "Delegating", "Running", "Calling",
        "Reading", "Writing", "Editing", "Creating", "Deleting",
        "Searching", "Finding", "Checking", "Validating", "Processing",
        "Fetching", "Loading", "Saving", "Updating", "Installing",
        "Building", "Compiling", "Testing", "Deploying",
        "There is an existing", "Already", "Successfully", "Failed to",
        "Starting", "Finishing", "Completed", "Done",
    ];

    for pattern in action_patterns {
        if trimmed.starts_with(pattern) {
            return true;
        }
    }

    // Also detect messages that look like tool operations (short with specific verbs)
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() <= 6 {
        if let Some(first) = words.first() {
            // Check for -ing verbs
            if first.ends_with("ing") && first.len() > 4 {
                return true;
            }
        }
    }

    false
}
